use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use wrfcoin_storage_common::error::StorageError;

pub struct ApiError(pub StorageError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let StorageError::NotInTestnet { ref provider } = self.0 {
            return (StatusCode::NOT_IMPLEMENTED, Json(json!({"unavailable": true, "provider": provider, "reason": "not in this testnet"}))).into_response();
        }
        let (status, error_message) = match self.0 {
            StorageError::NotInTestnet { .. } => unreachable!("handled above"),
            StorageError::NotConfigured { provider, reason } => (
                StatusCode::NOT_IMPLEMENTED,
                format!("{provider} not configured: {reason}"),
            ),
            StorageError::NotFound(ref msg) => (StatusCode::NOT_FOUND, msg.clone()),
            StorageError::PermissionDenied(ref msg) => (StatusCode::FORBIDDEN, msg.clone()),
            StorageError::CapacityExceeded => (
                StatusCode::INSUFFICIENT_STORAGE,
                "Storage capacity exceeded".to_string(),
            ),
            StorageError::InvalidData(ref msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            StorageError::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "Operation timed out".to_string(),
            ),
            StorageError::CostExceeded(ref msg) => (StatusCode::PAYMENT_REQUIRED, msg.clone()),
            StorageError::ConnectionFailed(ref msg) => {
                (StatusCode::SERVICE_UNAVAILABLE, msg.clone())
            }
            StorageError::ProviderError(ref msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, msg.clone())
            }
            StorageError::Internal(ref msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            StorageError::ConfigError(ref msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };

        let body = Json(json!({
            "error": error_message,
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}

impl<E> From<E> for ApiError
where
    E: Into<StorageError>,
{
    fn from(err: E) -> Self {
        ApiError(err.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn not_configured_maps_to_501() {
        let response = ApiError(StorageError::NotConfigured {
            provider: "arweave",
            reason: "no signer".into(),
        })
        .into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["status"], 501);
        assert_eq!(body["error"], "arweave not configured: no signer");
    }
}
