use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use wrfcoin_storage_common::{ContentId, Metadata, StorageStats, hashing::{ipfs_content_id, filecoin_content_id, sha256_digest}};
use wrfcoin_data_tiering::{AnalyticsInput, InMemoryMetrics, ProviderInput, StorageSnapshot};

use crate::error::ApiError;
use crate::state::AppState;

/// Maximum upload size: 256 MiB
const MAX_UPLOAD_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Deserialize)]
pub struct StoreRequest {
    /// Base64-encoded content to store
    pub data: String,
    /// Content type (MIME type)
    pub content_type: String,
    /// Optional provider to use; defaults to configured default
    pub provider: Option<String>,
    /// Optional metadata tags
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Serialize)]
pub struct StoreResponse {
    pub content_id: ContentId,
    pub provider: String,
    pub timestamp: u64,
    pub size: u64,
}

#[derive(Deserialize)]
pub struct RetrieveQuery {
    /// Optional provider hint (in case content was stored with a specific provider)
    pub provider: Option<String>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub providers: Vec<ProviderHealth>,
    pub timestamp: u64,
}

#[derive(Serialize)]
pub struct ProviderHealth {
    pub name: String,
    pub healthy: bool,
    pub latency_ms: Option<u64>,
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct StatsResponse {
    pub providers: Vec<ProviderStatsEntry>,
    pub timestamp: u64,
}

#[derive(Serialize)]
pub struct ProviderStatsEntry {
    pub name: String,
    pub stats: StorageStats,
}

/// GET /health - Check daemon and all provider health status
pub async fn health_check(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<HealthResponse>, ApiError> {
    let mut providers = Vec::new();

    for provider_name in app_state.provider_names() {
        let provider = app_state.get_provider(provider_name)?;
        let health = provider.health_check().await?;

        providers.push(ProviderHealth {
            name: health.provider.clone(),
            healthy: health.healthy,
            latency_ms: health.latency_ms,
            message: health.message.clone(),
        });
    }

    let overall_healthy = providers.iter().all(|p| p.healthy);

    Ok(Json(HealthResponse {
        status: if overall_healthy {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        },
        providers,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    }))
}

/// GET /stats - Get aggregate statistics for all providers
pub async fn stats(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<StatsResponse>, ApiError> {
    let mut providers = Vec::new();

    for provider_name in app_state.provider_names() {
        let provider = app_state.get_provider(provider_name)?;
        let stats = provider.stats().await?;

        providers.push(ProviderStatsEntry {
            name: provider_name.to_string(),
            stats,
        });
    }

    Ok(Json(StatsResponse {
        providers,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    }))
}

/// POST /api/store - Store data with metadata
///
/// Request body (JSON):
/// ```json
/// {
///   "data": "<base64-encoded bytes>",
///   "content_type": "application/json",
///   "provider": "ipfs",
///   "tags": {"key": "value"}
/// }
/// ```
pub async fn store(
    State(app_state): State<Arc<AppState>>,
    Json(req): Json<StoreRequest>,
) -> Result<(StatusCode, Json<StoreResponse>), ApiError> {
    // Decode base64 data
    let engine = base64::engine::general_purpose::STANDARD;
    let data = engine.decode(&req.data)
        .map_err(|e| ApiError(wrfcoin_storage_common::error::StorageError::InvalidData(
            format!("Invalid base64 encoding: {}", e),
        )))?;

    // Enforce maximum upload size
    if (data.len() as u64) > MAX_UPLOAD_BYTES {
        return Err(ApiError(wrfcoin_storage_common::error::StorageError::InvalidData(
            format!("Upload exceeds maximum size of {} bytes", MAX_UPLOAD_BYTES),
        )));
    }

    // Determine which provider to use
    let provider_name = req.provider.as_deref().unwrap_or(app_state.default_provider_name());

    let provider = app_state.get_provider(provider_name)?;

    // Create metadata
    let mut tags = req.tags.unwrap_or_default();
    tags.insert("stored_via".to_string(), "storage-node".to_string());

    let metadata = Metadata {
        content_type: req.content_type.clone(),
        size: data.len() as u64,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        tags,
    };

    // Store data
    let content_id = provider.store(&data, &metadata).await?;

    tracing::info!(
        "Stored {} bytes via {} provider: {}",
        data.len(),
        provider_name,
        content_id
    );

    Ok((
        StatusCode::CREATED,
        Json(StoreResponse {
            provider: content_id.provider.clone(),
            content_id,
            timestamp: metadata.timestamp,
            size: metadata.size,
        }),
    ))
}

/// GET /api/retrieve/:id - Retrieve stored data by content ID
///
/// URL parameters:
/// - `id`: Content ID in format "provider:hash" (e.g., "ipfs:QmXxxx")
///
/// Query parameters:
/// - `provider` (optional): Storage provider hint
pub async fn retrieve(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<RetrieveQuery>,
) -> Result<(StatusCode, Bytes), ApiError> {
    // Parse content ID
    let content_id = parse_content_id(&id)?;

    // Try to retrieve from specified provider or use default
    let provider_name = query.provider.as_deref().unwrap_or(&content_id.provider);
    let provider = app_state.get_provider(provider_name)?;

    let data = provider.retrieve(&content_id).await?;

    // Verify CID matches retrieved data
    verify_content_id(&content_id, &data)?;

    tracing::info!(
        "Retrieved {} bytes for content ID: {}",
        data.len(),
        content_id
    );

    Ok((StatusCode::OK, Bytes::from(data)))
}

/// DELETE /api/delete/:id - Delete stored data by content ID
///
/// URL parameters:
/// - `id`: Content ID in format "provider:hash"
pub async fn delete(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let content_id = parse_content_id(&id)?;
    let provider = app_state.get_provider(&content_id.provider)?;

    provider.delete(&content_id).await?;

    tracing::info!("Deleted content ID: {}", content_id);

    Ok(Json(json!({
        "status": "deleted",
        "content_id": content_id.to_string(),
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    })))
}

/// GET /api/exists/:id - Check whether content exists
///
/// URL parameters:
/// - `id`: Content ID in format "provider:hash"
pub async fn exists(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let content_id = parse_content_id(&id)?;
    let provider = app_state.get_provider(&content_id.provider)?;

    let exists = provider.exists(&content_id).await?;

    Ok(Json(json!({
        "content_id": content_id.to_string(),
        "exists": exists,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    })))
}

/// Parse content ID from "provider:hash" format
fn parse_content_id(id: &str) -> Result<ContentId, ApiError> {
    match id.split_once(':') {
        Some((provider, hash)) => {
            if provider.is_empty() || hash.is_empty() {
                return Err(ApiError(wrfcoin_storage_common::error::StorageError::InvalidData(
                    "Content ID must be in format 'provider:hash'".to_string(),
                )));
            }
            Ok(ContentId {
                provider: provider.to_string(),
                hash: hash.to_string(),
            })
        }
        None => Err(ApiError(wrfcoin_storage_common::error::StorageError::InvalidData(
            "Content ID must be in format 'provider:hash'".to_string(),
        ))),
    }
}

/// Verify that retrieved data matches the expected content ID
fn verify_content_id(expected: &ContentId, data: &[u8]) -> Result<(), ApiError> {
    let computed = match expected.provider.as_str() {
        "ipfs" => ipfs_content_id(data),
        "filecoin" => filecoin_content_id(data),
        "arweave" => {
            // For Arweave, verify using SHA-256 hex encoding
            let digest = sha256_digest(data);
            ContentId {
                provider: "arweave".to_string(),
                hash: format!("0x{}", hex::encode(digest)),
            }
        }
        _ => {
            // For unknown providers, we can't verify
            return Ok(());
        }
    };

    if computed.hash == expected.hash {
        Ok(())
    } else {
        Err(ApiError(wrfcoin_storage_common::error::StorageError::InvalidData(
            format!(
                "CID mismatch: expected {}, computed {}",
                expected.hash, computed.hash
            ),
        )))
    }
}

// ── Cost estimator handlers ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CostEstimateQuery {
    /// Payload size in bytes.
    pub size_bytes: u64,
    /// Storage duration in seconds (default: 86400 = 1 day).
    pub duration_secs: Option<u64>,
}

#[derive(Deserialize)]
pub struct CheapestQuery {
    /// Payload size in bytes.
    pub size_bytes: u64,
    /// Storage duration in seconds (default: 86400 = 1 day).
    pub duration_secs: Option<u64>,
    /// Optional maximum total-cost budget in the estimate's currency.
    pub max_budget: Option<f64>,
}

/// GET /api/cost/estimate?size_bytes=N&duration_secs=N
///
/// Returns cost estimates for all configured providers, sorted cheapest-first
/// by `total_cost`.  Providers that share the same currency are directly
/// comparable; cross-currency entries are omitted from the sorted result.
pub async fn cost_estimate(
    State(app_state): State<Arc<AppState>>,
    Query(q): Query<CostEstimateQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let duration_secs = q.duration_secs.unwrap_or(86_400);
    let ranked = app_state.cost_estimator.compare(q.size_bytes, duration_secs).await;

    let estimates: Vec<serde_json::Value> = ranked
        .into_iter()
        .map(|(_, est)| serde_json::to_value(&est).unwrap_or(json!(null)))
        .collect();

    Ok(Json(json!({
        "size_bytes": q.size_bytes,
        "duration_secs": duration_secs,
        "estimates": estimates,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    })))
}

/// GET /api/cost/cheapest?size_bytes=N&duration_secs=N&max_budget=N
///
/// Returns the cheapest healthy provider for the requested size and duration,
/// subject to an optional `max_budget` ceiling.  Returns 404 when no provider
/// passes the health and budget constraints.
pub async fn cost_cheapest(
    State(app_state): State<Arc<AppState>>,
    Query(q): Query<CheapestQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let duration_secs = q.duration_secs.unwrap_or(86_400);

    // Build a health map from current provider health checks.
    let provider_names: Vec<String> = app_state.provider_names()
        .into_iter()
        .map(String::from)
        .collect();
    let mut health_map: HashMap<String, bool> = HashMap::new();
    for name in &provider_names {
        if let Ok(provider) = app_state.get_provider(name) {
            let healthy = provider.health_check().await
                .map(|h| h.healthy)
                .unwrap_or(false);
            health_map.insert(name.clone(), healthy);
        }
    }

    let result = app_state
        .cost_estimator
        .cheapest_provider(q.size_bytes, duration_secs, &health_map, q.max_budget)
        .await;

    match result {
        Some(est) => Ok(Json(json!({
            "size_bytes": q.size_bytes,
            "duration_secs": duration_secs,
            "cheapest": serde_json::to_value(&est).unwrap_or(json!(null)),
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }))),
        None => Err(ApiError(wrfcoin_storage_common::error::StorageError::ProviderError(
            "No healthy provider found within budget constraints".to_string(),
        ))),
    }
}

// ── Analytics handlers ─────────────────────────────────────────────────────────

/// GET /api/analytics/snapshot
///
/// Builds a `StorageSnapshot` from current in-memory metrics and returns it as
/// JSON.  The snapshot is also pushed onto the ring-buffer history (capacity 60).
pub async fn analytics_snapshot(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<StorageSnapshot>, ApiError> {
    let provider_names: Vec<String> = app_state.provider_names()
        .into_iter()
        .map(String::from)
        .collect();

    // Phase 1 — async: get provider health (no Mutex locks held across await).
    let mut health_map: HashMap<String, bool> = HashMap::new();
    for name in &provider_names {
        if let Ok(provider) = app_state.get_provider(name) {
            let healthy = provider.health_check().await
                .map(|h| h.healthy)
                .unwrap_or(false);
            health_map.insert(name.clone(), healthy);
        }
    }

    // Phase 2 — sync: collect cloned stats (lock acquired + released immediately).
    let stats_clones: Vec<StorageStats> = {
        let stats_map = app_state.provider_stats.lock().unwrap();
        provider_names.iter()
            .map(|name| stats_map.get(name.as_str()).cloned().unwrap_or_default())
            .collect()
    };

    // Collect Arc<InMemoryMetrics> — no lock needed (provider_metrics is Arc<HashMap>).
    let metrics_arcs: Vec<Arc<InMemoryMetrics>> = provider_names.iter()
        .map(|name| {
            app_state.provider_metrics.get(name.as_str())
                .cloned()
                .unwrap_or_else(|| Arc::new(InMemoryMetrics::new()))
        })
        .collect();

    // Phase 3 — sync: build AnalyticsInput and take snapshot (no await).
    let snap = {
        let budget = app_state.budget.lock().unwrap();
        let domain_index = app_state.domain_index.lock().unwrap();
        let failover = app_state.failover_metrics.lock().unwrap();

        let provider_inputs: Vec<ProviderInput<'_>> = provider_names.iter()
            .enumerate()
            .map(|(i, name)| ProviderInput {
                name: name.as_str(),
                healthy: health_map.get(name.as_str()).copied().unwrap_or(true),
                stats: &stats_clones[i],
                metrics: &metrics_arcs[i],
                cost_to_date: 0.0,
            })
            .collect();

        let input = AnalyticsInput {
            providers: provider_inputs,
            budget: &*budget,
            tiering_config: &app_state.tiering_config,
            domain_index: &*domain_index,
            sla_statuses: &[],
            failover: &*failover,
            tier_bytes: None,
            domain_bytes: None,
            timestamp: 0,
        };

        app_state.analytics.lock().unwrap().snapshot(&input)
    };

    Ok(Json(snap))
}

/// GET /api/analytics/history
///
/// Returns the full ring-buffer of past snapshots (up to 60 entries, oldest
/// first) as a JSON array.
pub async fn analytics_history(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let history: Vec<StorageSnapshot> = app_state
        .analytics
        .lock()
        .unwrap()
        .history()
        .iter()
        .cloned()
        .collect();

    Ok(Json(json!({
        "count": history.len(),
        "snapshots": history,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    })))
}
