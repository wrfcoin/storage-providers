use async_trait::async_trait;

use crate::error::StorageError;
use crate::types::{ContentId, HealthStatus, Metadata, StorageStats};

/// Core trait that all storage providers must implement.
///
/// Each method is async to accommodate network I/O with remote storage backends.
#[async_trait]
pub trait StorageProvider: Send + Sync {
    /// Store data with the given metadata and return a content identifier.
    async fn store(&self, data: &[u8], metadata: &Metadata) -> Result<ContentId, StorageError>;

    /// Retrieve data by its content identifier.
    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError>;

    /// Check whether content exists in this provider.
    async fn exists(&self, id: &ContentId) -> Result<bool, StorageError>;

    /// Delete content from this provider.
    async fn delete(&self, id: &ContentId) -> Result<(), StorageError>;

    /// Perform a health check on the provider.
    async fn health_check(&self) -> Result<HealthStatus, StorageError>;

    /// Return aggregate usage statistics.
    async fn stats(&self) -> Result<StorageStats, StorageError>;
}

/// Trait for computing content-addressed identifiers from raw data.
pub trait ContentAddressed {
    /// Compute a deterministic content identifier from the given bytes.
    fn content_id(data: &[u8]) -> ContentId;
}
