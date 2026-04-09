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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("wrfcoin_storage_daemon=debug".parse()?),
        )
        .json()
        .init();

    // Initialize storage providers
    let app_state = Arc::new(state::AppState::new().await?);

    // Build router with REST endpoints
    let app = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/stats", get(handlers::stats))
        .route("/api/store", post(handlers::store))
        .route("/api/retrieve/:id", get(handlers::retrieve))
        .route("/api/delete/:id", delete(handlers::delete))
        .route("/api/exists/:id", get(handlers::exists))
        .with_state(app_state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 7001));
    tracing::info!("Storage daemon listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
