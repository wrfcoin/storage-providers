mod handlers;
mod state;
mod error;

use axum::{
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber;
use serde::{Deserialize, Serialize};
use std::fs;
use prometheus::{Counter, Registry};

/// Configuration for storage node from TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageNodeConfig {
    /// Server bind address and port
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    /// Storage tier configuration (e.g., "hot", "cold", "archive")
    #[serde(default = "default_storage_tier")]
    pub storage_tier: String,
    /// Provider endpoints (e.g., IPFS URL, Filecoin endpoint)
    #[serde(default)]
    pub provider_endpoints: ProviderEndpoints,
    /// Cache TTL in seconds
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderEndpoints {
    pub ipfs: Option<String>,
    pub filecoin: Option<String>,
    pub arweave: Option<String>,
}

fn default_bind_addr() -> String {
    "0.0.0.0:7001".to_string()
}

fn default_storage_tier() -> String {
    "hot".to_string()
}

fn default_cache_ttl() -> u64 {
    3600
}

impl Default for StorageNodeConfig {
    fn default() -> Self {
        Self {
            bind_addr: default_bind_addr(),
            storage_tier: default_storage_tier(),
            provider_endpoints: ProviderEndpoints::default(),
            cache_ttl_secs: default_cache_ttl(),
        }
    }
}

impl StorageNodeConfig {
    /// Load configuration from TOML file, with env var override.
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = std::env::var("STORAGE_NODE_CONFIG")
            .unwrap_or_else(|_| "storage-node-config.toml".to_string());

        if let Ok(content) = fs::read_to_string(&config_path) {
            Ok(toml::from_str(&content)?)
        } else {
            // Use defaults if config file not found
            Ok(Self::default())
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("wrfcoin_storage_node=debug".parse()?),
        )
        .json()
        .init();

    // Load configuration
    let config = StorageNodeConfig::load()?;
    tracing::info!(
        "Storage node starting with config: tier={}, addr={}",
        config.storage_tier,
        config.bind_addr
    );

    // Initialize Prometheus metrics
    let registry = Arc::new(Registry::new());
    let requests_counter = Counter::new("storage_node_requests_total", "Total requests handled")
        .map_err(|e| format!("Failed to create counter: {}", e))?;
    registry.register(Box::new(requests_counter.clone()))?;

    // Initialize storage providers
    let app_state = Arc::new(state::AppState::new().await?);

    // Clone registry for metrics endpoint
    let registry_clone = registry.clone();

    // Build router with REST endpoints
    let app = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/stats", get(handlers::stats))
        .route("/metrics", get(move || {
            let reg = registry_clone.clone();
            async move {
                use prometheus::TextEncoder;
                TextEncoder::new()
                    .encode_to_string(&reg.gather())
                    .unwrap_or_default()
            }
        }))
        .route("/api/store", post(handlers::store))
        .route("/api/retrieve/:id", get(handlers::retrieve))
        .route("/api/delete/:id", delete(handlers::delete))
        .route("/api/exists/:id", get(handlers::exists))
        .with_state(app_state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    // Parse bind address
    let addr: SocketAddr = config.bind_addr.parse()?;
    tracing::info!("Storage node listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    // Setup graceful shutdown on SIGTERM
    let shutdown_signal = async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate())
                .expect("Failed to setup SIGTERM handler");
            sigterm.recv().await;
            tracing::info!("SIGTERM received, initiating graceful shutdown...");
        }
        #[cfg(not(unix))]
        {
            // On non-Unix systems, use Ctrl+C
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Shutdown signal received, initiating graceful shutdown...");
        }
    };

    // Start server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    tracing::info!("Storage node shut down gracefully");
    Ok(())
}
