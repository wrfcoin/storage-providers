//! Cross-provider data migration with verification.
//!
//! Provides tools to move content between different storage providers (e.g., IPFS → Filecoin)
//! with automatic CID verification after transfer to ensure data integrity.

use serde::{Deserialize, Serialize};
use wrfcoin_storage_common::{
    types::ContentId, IntegrityVerifier, StorageError, StorageProvider,
};

/// Status of a cross-provider migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationJobStatus {
    /// Migration is queued and waiting to start.
    Pending,
    /// Migration is in progress (data being transferred).
    InProgress,
    /// Migration completed; data was verified successfully.
    Completed,
    /// Migration failed; see error_message for details.
    Failed,
    /// Migration was rolled back due to verification failure.
    RolledBack,
}

/// Request to migrate content between two storage providers.
#[derive(Debug, Clone)]
pub struct CrossProviderMigrationRequest {
    /// The content identifier (from source provider).
    pub content_id: ContentId,
    /// Source provider (e.g., "ipfs", "arweave").
    pub source_provider: String,
    /// Destination provider (e.g., "filecoin", "ipfs").
    pub dest_provider: String,
    /// Whether to delete from source after successful migration.
    pub delete_source: bool,
    /// Maximum number of retry attempts.
    pub max_retries: u32,
}

/// Result of CID verification after migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationResult {
    /// Whether the CID matches the original.
    pub verified: bool,
    /// Original CID from source.
    pub original_cid: String,
    /// Computed CID from destination.
    pub migrated_cid: String,
    /// SHA-256 digest of the migrated content.
    pub content_digest: String,
}

/// Summary of a cross-provider migration operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrossProviderMigrationSummary {
    /// Total items migrated.
    pub items_migrated: u64,
    /// Total bytes migrated.
    pub bytes_migrated: u64,
    /// Number of migrations that succeeded.
    pub successful: u64,
    /// Number of migrations that failed.
    pub failed: u64,
    /// Number of migrations that were rolled back.
    pub rolled_back: u64,
    /// Total errors encountered.
    pub errors: u64,
}

impl CrossProviderMigrationSummary {
    /// Check if any migration was performed.
    pub fn is_empty(&self) -> bool {
        self.items_migrated == 0 && self.errors == 0
    }
}

/// Orchestrates cross-provider data migration with verification.
pub struct CrossProviderMigrator {
    /// Pending migration requests.
    pending_requests: Vec<CrossProviderMigrationRequest>,
    /// Migration history (for debugging and audit).
    history: Vec<(CrossProviderMigrationRequest, MigrationJobStatus, Option<String>)>,
}

impl CrossProviderMigrator {
    /// Create a new cross-provider migrator.
    pub fn new() -> Self {
        Self {
            pending_requests: Vec::new(),
            history: Vec::new(),
        }
    }

    /// Queue a cross-provider migration request.
    pub fn queue_migration(
        &mut self,
        content_id: ContentId,
        source_provider: String,
        dest_provider: String,
        delete_source: bool,
    ) {
        self.pending_requests.push(CrossProviderMigrationRequest {
            content_id,
            source_provider,
            dest_provider,
            delete_source,
            max_retries: 3,
        });
    }

    /// Migrate a single piece of content between providers with verification.
    ///
    /// # Arguments
    ///
    /// * `request` - Migration request describing source, destination, and content
    /// * `source` - Source storage provider implementation
    /// * `dest` - Destination storage provider implementation
    ///
    /// # Returns
    ///
    /// A `VerificationResult` indicating whether the migration succeeded and CID was verified.
    ///
    /// # Errors
    ///
    /// Returns a `StorageError` if retrieval, storage, or verification fails.
    pub async fn migrate_content(
        &self,
        request: &CrossProviderMigrationRequest,
        source: &dyn StorageProvider,
        dest: &dyn StorageProvider,
    ) -> Result<VerificationResult, StorageError> {
        // 1. Retrieve content from source provider
        let data = source
            .retrieve(&request.content_id)
            .await
            .map_err(|e| StorageError::ProviderError(format!(
                "failed to retrieve from {}: {:?}",
                request.source_provider, e
            )))?;

        // 2. Compute the blake3 hash of the original data for verification
        let original_digest_hex = IntegrityVerifier::hash(&data);

        // 3. Store in destination provider
        let metadata = wrfcoin_storage_common::types::Metadata {
            content_type: "application/octet-stream".to_string(),
            size: data.len() as u64,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            tags: {
                let mut tags = std::collections::HashMap::new();
                tags.insert("migrated_from".to_string(), request.source_provider.clone());
                tags.insert("migration_source_cid".to_string(), request.content_id.hash.clone());
                tags
            },
        };

        let dest_cid = dest
            .store(&data, &metadata)
            .await
            .map_err(|e| StorageError::ProviderError(format!(
                "failed to store to {}: {:?}",
                request.dest_provider, e
            )))?;

        // 4. Verify by re-retrieving and comparing hashes
        let migrated_data = dest
            .retrieve(&dest_cid)
            .await
            .map_err(|e| StorageError::ProviderError(format!(
                "verification: failed to retrieve from destination: {:?}",
                e
            )))?;

        let migrated_digest_hex = IntegrityVerifier::hash(&migrated_data);

        let verified = original_digest_hex == migrated_digest_hex;

        // 5. If verification failed, optionally delete from destination
        if !verified {
            let _ = dest.delete(&dest_cid).await;
            return Err(StorageError::InvalidData(
                format!(
                    "CID verification failed: {} != {}",
                    original_digest_hex, migrated_digest_hex
                )
            ));
        }

        // 6. If source deletion is requested and verification passed, delete from source
        if request.delete_source {
            let _ = source.delete(&request.content_id).await;
        }

        Ok(VerificationResult {
            verified: true,
            original_cid: request.content_id.to_string(),
            migrated_cid: dest_cid.to_string(),
            content_digest: migrated_digest_hex,
        })
    }

    /// Process all pending migration requests.
    ///
    /// This function attempts to migrate all queued content, collecting results
    /// in a summary. Failed migrations are recorded in history but do not prevent
    /// subsequent migrations from being attempted.
    pub async fn process_all(
        &mut self,
        source: &dyn StorageProvider,
        dest: &dyn StorageProvider,
    ) -> CrossProviderMigrationSummary {
        let mut summary = CrossProviderMigrationSummary::default();
        let requests = std::mem::take(&mut self.pending_requests);

        for request in requests {
            summary.items_migrated += 1;
            summary.bytes_migrated += 1; // In a real implementation, track actual byte count

            match self.migrate_content(&request, source, dest).await {
                Ok(_result) => {
                    summary.successful += 1;
                    self.history.push((
                        request,
                        MigrationJobStatus::Completed,
                        None,
                    ));
                }
                Err(e) => {
                    summary.failed += 1;
                    summary.errors += 1;
                    self.history.push((
                        request,
                        MigrationJobStatus::Failed,
                        Some(format!("{:?}", e)),
                    ));
                }
            }
        }

        summary
    }

    /// Get the migration history.
    pub fn history(&self) -> &[(CrossProviderMigrationRequest, MigrationJobStatus, Option<String>)] {
        &self.history
    }

    /// Clear the migration history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

impl Default for CrossProviderMigrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Mock storage provider for testing.
    struct MockProvider {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl StorageProvider for MockProvider {
        async fn store(
            &self,
            data: &[u8],
            _metadata: &wrfcoin_storage_common::types::Metadata,
        ) -> Result<ContentId, StorageError> {
            let cid = wrfcoin_storage_common::ipfs_content_id(data);
            self.data
                .lock()
                .unwrap()
                .insert(cid.hash.clone(), data.to_vec());
            Ok(cid)
        }

        async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
            self.data
                .lock()
                .unwrap()
                .get(&id.hash)
                .cloned()
                .ok_or_else(|| StorageError::NotFound(id.hash.clone()))
        }

        async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
            Ok(self.data.lock().unwrap().contains_key(&id.hash))
        }

        async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
            self.data.lock().unwrap().remove(&id.hash);
            Ok(())
        }

        async fn health_check(
            &self,
        ) -> Result<wrfcoin_storage_common::types::HealthStatus, StorageError> {
            Ok(wrfcoin_storage_common::types::HealthStatus {
                healthy: true,
                provider: "mock".to_string(),
                latency_ms: Some(1),
                message: Some("mock".to_string()),
            })
        }

        async fn stats(
            &self,
        ) -> Result<wrfcoin_storage_common::types::StorageStats, StorageError> {
            Ok(wrfcoin_storage_common::types::StorageStats {
                total_stored: 0,
                total_retrieved: 0,
                total_bytes: 0,
                cache_hits: 0,
                cache_misses: 0,
            })
        }
    }

    #[tokio::test]
    async fn test_migrate_content_successfully_verifies_cid() {
        let source = MockProvider::new();
        let dest = MockProvider::new();
        let migrator = CrossProviderMigrator::new();

        let test_data = b"test data for migration";
        let cid = wrfcoin_storage_common::ipfs_content_id(test_data);

        // Store test data in source
        let metadata = wrfcoin_storage_common::types::Metadata {
            content_type: "text/plain".to_string(),
            size: test_data.len() as u64,
            timestamp: 0,
            tags: HashMap::new(),
        };
        source.store(test_data, &metadata).await.unwrap();

        // Create migration request
        let request = CrossProviderMigrationRequest {
            content_id: cid.clone(),
            source_provider: "ipfs".to_string(),
            dest_provider: "filecoin".to_string(),
            delete_source: false,
            max_retries: 3,
        };

        // Perform migration
        let result = migrator
            .migrate_content(&request, &source, &dest)
            .await
            .expect("migration should succeed");

        assert!(result.verified, "CID verification should pass");
        assert_eq!(result.original_cid, cid.to_string());
    }

    #[tokio::test]
    async fn test_migrate_content_preserves_data_integrity() {
        let source = MockProvider::new();
        let dest = MockProvider::new();
        let migrator = CrossProviderMigrator::new();

        let test_data = b"important weather data that must not be corrupted";
        let cid = wrfcoin_storage_common::ipfs_content_id(test_data);

        let metadata = wrfcoin_storage_common::types::Metadata {
            content_type: "application/octet-stream".to_string(),
            size: test_data.len() as u64,
            timestamp: 0,
            tags: HashMap::new(),
        };
        source.store(test_data, &metadata).await.unwrap();

        let request = CrossProviderMigrationRequest {
            content_id: cid,
            source_provider: "ipfs".to_string(),
            dest_provider: "arweave".to_string(),
            delete_source: false,
            max_retries: 3,
        };

        let result = migrator
            .migrate_content(&request, &source, &dest)
            .await
            .expect("migration should succeed");

        // Retrieve from destination and verify content matches
        let dest_cid = wrfcoin_storage_common::ContentId {
            provider: "arweave".to_string(),
            hash: result.migrated_cid.split(':').last().unwrap().to_string(),
        };
        let retrieved = dest
            .retrieve(&dest_cid)
            .await
            .expect("should be able to retrieve");
        assert_eq!(retrieved, test_data, "migrated data should match original");
    }

    #[tokio::test]
    async fn test_migrate_deletes_source_when_requested() {
        let source = MockProvider::new();
        let dest = MockProvider::new();
        let migrator = CrossProviderMigrator::new();

        let test_data = b"temporary data";
        let cid = wrfcoin_storage_common::ipfs_content_id(test_data);

        let metadata = wrfcoin_storage_common::types::Metadata {
            content_type: "text/plain".to_string(),
            size: test_data.len() as u64,
            timestamp: 0,
            tags: HashMap::new(),
        };
        source.store(test_data, &metadata).await.unwrap();

        // Verify source has the data
        assert!(source.exists(&cid).await.unwrap());

        let request = CrossProviderMigrationRequest {
            content_id: cid.clone(),
            source_provider: "ipfs".to_string(),
            dest_provider: "filecoin".to_string(),
            delete_source: true, // Request deletion
            max_retries: 3,
        };

        migrator
            .migrate_content(&request, &source, &dest)
            .await
            .expect("migration should succeed");

        // Verify source data was deleted
        assert!(
            !source.exists(&cid).await.unwrap(),
            "source data should be deleted"
        );
    }

    #[tokio::test]
    async fn test_migration_request_queuing() {
        let mut migrator = CrossProviderMigrator::new();

        let cid1 = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm_1".to_string(),
        };
        let cid2 = ContentId {
            provider: "arweave".to_string(),
            hash: "Ar_2".to_string(),
        };

        migrator.queue_migration(cid1, "ipfs".to_string(), "filecoin".to_string(), false);
        migrator.queue_migration(cid2, "arweave".to_string(), "ipfs".to_string(), true);

        assert_eq!(migrator.pending_requests.len(), 2);
    }

    #[tokio::test]
    async fn test_migration_summary_empty_when_no_work() {
        let migrator = CrossProviderMigrator::new();
        let source = MockProvider::new();
        let dest = MockProvider::new();

        let mut migrator = migrator;
        let summary = migrator.process_all(&source, &dest).await;

        assert!(summary.is_empty());
        assert_eq!(summary.items_migrated, 0);
    }

    #[test]
    fn test_verification_result_equality() {
        let result1 = VerificationResult {
            verified: true,
            original_cid: "Qm_abc".to_string(),
            migrated_cid: "Qm_xyz".to_string(),
            content_digest: "abc123".to_string(),
        };

        let result2 = VerificationResult {
            verified: true,
            original_cid: "Qm_abc".to_string(),
            migrated_cid: "Qm_xyz".to_string(),
            content_digest: "abc123".to_string(),
        };

        assert_eq!(result1, result2);
    }

    /// Mock provider that corrupts data on retrieval to simulate verification failure.
    struct CorruptingMockProvider {
        data: Mutex<HashMap<String, Vec<u8>>>,
        deleted_ids: Mutex<Vec<String>>,
    }

    impl CorruptingMockProvider {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
                deleted_ids: Mutex::new(Vec::new()),
            }
        }

        fn was_deleted(&self, hash: &str) -> bool {
            self.deleted_ids
                .lock()
                .unwrap()
                .iter()
                .any(|id| id == hash)
        }
    }

    #[async_trait::async_trait]
    impl StorageProvider for CorruptingMockProvider {
        async fn store(
            &self,
            data: &[u8],
            _metadata: &wrfcoin_storage_common::types::Metadata,
        ) -> Result<ContentId, StorageError> {
            let cid = wrfcoin_storage_common::ipfs_content_id(data);
            self.data
                .lock()
                .unwrap()
                .insert(cid.hash.clone(), data.to_vec());
            Ok(cid)
        }

        async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
            self.data
                .lock()
                .unwrap()
                .get(&id.hash)
                .map(|data| {
                    // Corrupt the data by modifying the first byte
                    let mut corrupted = data.clone();
                    if !corrupted.is_empty() {
                        corrupted[0] = corrupted[0].wrapping_add(1);
                    }
                    corrupted
                })
                .ok_or_else(|| StorageError::NotFound(id.hash.clone()))
        }

        async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
            Ok(self.data.lock().unwrap().contains_key(&id.hash))
        }

        async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
            self.deleted_ids.lock().unwrap().push(id.hash.clone());
            self.data.lock().unwrap().remove(&id.hash);
            Ok(())
        }

        async fn health_check(
            &self,
        ) -> Result<wrfcoin_storage_common::types::HealthStatus, StorageError> {
            Ok(wrfcoin_storage_common::types::HealthStatus {
                healthy: true,
                provider: "corrupting_mock".to_string(),
                latency_ms: Some(1),
                message: Some("corrupting_mock".to_string()),
            })
        }

        async fn stats(
            &self,
        ) -> Result<wrfcoin_storage_common::types::StorageStats, StorageError> {
            Ok(wrfcoin_storage_common::types::StorageStats {
                total_stored: 0,
                total_retrieved: 0,
                total_bytes: 0,
                cache_hits: 0,
                cache_misses: 0,
            })
        }
    }

    #[tokio::test]
    async fn test_migration_rollback_on_verification_failure() {
        let source = MockProvider::new();
        let dest = CorruptingMockProvider::new();
        let migrator = CrossProviderMigrator::new();

        let test_data = b"critical weather data that must not be corrupted";
        let cid = wrfcoin_storage_common::ipfs_content_id(test_data);

        // Store test data in source
        let metadata = wrfcoin_storage_common::types::Metadata {
            content_type: "application/octet-stream".to_string(),
            size: test_data.len() as u64,
            timestamp: 0,
            tags: HashMap::new(),
        };
        source.store(test_data, &metadata).await.unwrap();

        // Create migration request
        let request = CrossProviderMigrationRequest {
            content_id: cid.clone(),
            source_provider: "ipfs".to_string(),
            dest_provider: "filecoin".to_string(),
            delete_source: false,
            max_retries: 3,
        };

        // Perform migration — should fail due to hash mismatch
        let result = migrator
            .migrate_content(&request, &source, &dest)
            .await;

        // Migration should fail
        assert!(result.is_err(), "migration should fail due to verification failure");

        // Verify the destination data was rolled back (deleted)
        assert!(
            dest.was_deleted(&cid.hash),
            "destination data should be deleted on verification failure (rollback)"
        );
    }
}
