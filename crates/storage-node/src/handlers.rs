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
use wrfcoin_storage_common::{ContentId, Metadata, StorageStats};

use crate::error::ApiError;
use crate::state::AppState;

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

    // Determine which provider to use
    let provider_name = req.provider.as_deref().unwrap_or(app_state.default_provider_name());

    let provider = app_state.get_provider(provider_name)?;

    // Create metadata
    let mut tags = req.tags.unwrap_or_default();
    tags.insert("stored_via".to_string(), "storage-daemon".to_string());

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
