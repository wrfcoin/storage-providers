use thiserror::Error;

/// Unified error type for all storage provider operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Provider excluded from the zero-budget testnet profile.
    #[error("{provider}: not in this testnet")]
    NotInTestnet { provider: String },
    /// The provider cannot perform writes in this build.
    #[error("{provider} not configured: {reason}")]
    NotConfigured {
        provider: &'static str,
        reason: String,
    },

    /// Failed to connect to the storage backend.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// The requested content was not found.
    #[error("content not found: {0}")]
    NotFound(String),

    /// The caller lacks permission for the requested operation.
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// The storage provider has reached its capacity limit.
    #[error("storage capacity exceeded")]
    CapacityExceeded,

    /// The supplied data is invalid or malformed.
    #[error("invalid data: {0}")]
    InvalidData(String),

    /// A provider-specific error occurred.
    #[error("provider error: {0}")]
    ProviderError(String),

    /// The operation timed out.
    #[error("operation timed out")]
    Timeout,

    /// The operation would exceed the configured cost limit.
    #[error("cost exceeded: {0}")]
    CostExceeded(String),

    /// An internal error (e.g. lock poisoning).
    #[error("internal error: {0}")]
    Internal(String),

    /// A configuration error (e.g. missing or invalid settings).
    #[error("config error: {0}")]
    ConfigError(String),
}
