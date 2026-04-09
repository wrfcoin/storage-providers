//! Forecast verification receipt archiver.
//!
//! Archives receipts to decentralized storage with CID anchoring.

use wrfcoin_storage_common::{
    types::{ContentId, Metadata},
    StorageProvider,
};
use thiserror::Error;

use crate::{canonical_json, ForecastVerificationReceipt};

/// Errors that can occur during receipt archival.
#[derive(Debug, Clone, Error)]
pub enum ArchiveError {
    /// Serialization of the receipt failed.
    #[error("serialization error: {0}")]
    SerializationError(String),
    /// Storage operation failed.
    #[error("storage error: {0}")]
    StorageError(String),
    /// CID computation failed.
    #[error("cid error: {0}")]
    CidError(String),
}

/// Archive a forecast verification receipt to decentralized storage.
///
/// This function:
/// 1. Serializes the receipt to canonical JSON (deterministic byte sequence)
/// 2. Computes the IPFS CID from the canonical bytes
/// 3. Stores the receipt via the given [`StorageProvider`]
/// 4. Returns the computed CID and an optional Arweave transaction ID
///
/// The canonical serialization ensures that the same receipt always produces
/// the same CID, enabling verification and deduplication.
///
/// # Arguments
///
/// * `provider` - A storage provider implementation (e.g., IPFS, Arweave, Filecoin)
/// * `receipt` - The forecast verification receipt to archive
///
/// # Returns
///
/// A tuple `(cid, arweave_tx_id)` where:
/// - `cid` is the computed IPFS content identifier
/// - `arweave_tx_id` is an optional Arweave transaction ID (if the provider supports it)
///
/// # Errors
///
/// Returns [`ArchiveError`] if serialization or storage fails.
///
/// # Example
///
/// ```ignore
/// use receipt_archiver::{ForecastVerificationReceipt, ForecastAccuracyMetrics, archive_receipt};
///
/// let metrics = ForecastAccuracyMetrics {
///     mae: 0.5,
///     rmse: 0.7,
///     bias: -0.1,
///     skill_score: 0.85,
/// };
///
/// let receipt = ForecastVerificationReceipt::new(
///     "a".repeat(64),
///     "commit-001".to_string(),
///     "b".repeat(64),
///     metrics,
///     1_000_000,
///     "nonce-xyz".to_string(),
/// );
///
/// let (cid, _) = archive_receipt(&provider, &receipt).await?;
/// println!("Archived receipt at {}", cid);
/// # Ok::<(), ArchiveError>(())
/// ```
pub async fn archive_receipt(
    provider: &dyn StorageProvider,
    receipt: &ForecastVerificationReceipt,
) -> Result<(ContentId, Option<String>), ArchiveError> {
    // 1. Serialize the receipt to canonical JSON.
    let canonical_bytes = canonical_json(receipt)
        .map_err(|e| ArchiveError::SerializationError(e.to_string()))?;

    // 2. Prepare metadata for storage (including tags for retrieval and verification).
    let metadata = Metadata {
        content_type: "application/json".to_string(),
        size: canonical_bytes.len() as u64,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| ArchiveError::CidError(format!("timestamp error: {}", e)))?
            .as_secs(),
        tags: {
            let mut tags = std::collections::HashMap::new();
            tags.insert("type".to_string(), "forecast_receipt".to_string());
            tags.insert("manifest_digest".to_string(), receipt.manifest_digest.clone());
            tags.insert("commit_id".to_string(), receipt.commit_id.clone());
            tags.insert("oracle_id".to_string(), receipt.oracle_id.clone());
            tags
        },
    };

    // 3. Store the canonical bytes via the provider (which computes the CID).
    let cid = provider
        .store(&canonical_bytes, &metadata)
        .await
        .map_err(|e| ArchiveError::StorageError(format!("{:?}", e)))?;

    // 4. Note: Arweave transaction ID would be returned if a dedicated Arweave
    // integration were implemented. For now, this returns None.
    Ok((cid, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use wrfcoin_storage_common::ipfs_content_id;

    /// Mock storage provider for testing.
    struct MockStorageProvider {
        stored_data: std::sync::Mutex<HashMap<String, Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl StorageProvider for MockStorageProvider {
        async fn store(
            &self,
            data: &[u8],
            _metadata: &Metadata,
        ) -> Result<ContentId, wrfcoin_storage_common::StorageError> {
            let cid = ipfs_content_id(data);
            self.stored_data
                .lock()
                .unwrap()
                .insert(cid.hash.clone(), data.to_vec());
            Ok(cid)
        }

        async fn retrieve(
            &self,
            id: &ContentId,
        ) -> Result<Vec<u8>, wrfcoin_storage_common::StorageError> {
            self.stored_data
                .lock()
                .unwrap()
                .get(&id.hash)
                .cloned()
                .ok_or_else(|| {
                    wrfcoin_storage_common::StorageError::NotFound(id.hash.clone())
                })
        }

        async fn exists(
            &self,
            id: &ContentId,
        ) -> Result<bool, wrfcoin_storage_common::StorageError> {
            Ok(self.stored_data.lock().unwrap().contains_key(&id.hash))
        }

        async fn delete(
            &self,
            id: &ContentId,
        ) -> Result<(), wrfcoin_storage_common::StorageError> {
            self.stored_data.lock().unwrap().remove(&id.hash);
            Ok(())
        }

        async fn health_check(
            &self,
        ) -> Result<wrfcoin_storage_common::types::HealthStatus, wrfcoin_storage_common::StorageError> {
            Ok(wrfcoin_storage_common::types::HealthStatus {
                healthy: true,
                provider: "mock".to_string(),
                latency_ms: Some(1),
                message: Some("mock provider".to_string()),
            })
        }

        async fn stats(
            &self,
        ) -> Result<wrfcoin_storage_common::types::StorageStats, wrfcoin_storage_common::StorageError> {
            Ok(wrfcoin_storage_common::types::StorageStats {
                total_stored: 0,
                total_retrieved: 0,
                total_bytes: 0,
                cache_hits: 0,
                cache_misses: 0,
            })
        }
    }

    fn sample_receipt() -> ForecastVerificationReceipt {
        use crate::receipt::ForecastAccuracyMetrics;
        ForecastVerificationReceipt::new(
            "a".repeat(64),
            "commit-001".to_string(),
            "b".repeat(64),
            ForecastAccuracyMetrics {
                mae: 0.5,
                rmse: 0.7,
                bias: -0.1,
                skill_score: 0.85,
            },
            1_000_000,
            "nonce-xyz".to_string(),
        )
    }

    #[tokio::test]
    async fn test_archive_receipt_stores_data() {
        let provider = MockStorageProvider {
            stored_data: std::sync::Mutex::new(HashMap::new()),
        };
        let receipt = sample_receipt();

        let (cid, arweave_tx) = archive_receipt(&provider, &receipt)
            .await
            .expect("archival should succeed");

        assert_eq!(cid.provider, "ipfs");
        assert!(cid.hash.starts_with("Qm"));
        assert!(arweave_tx.is_none());

        // Verify the data was stored.
        assert!(provider.exists(&cid).await.unwrap());
    }

    #[tokio::test]
    async fn test_archive_receipt_deterministic_cid() {
        let provider = MockStorageProvider {
            stored_data: std::sync::Mutex::new(HashMap::new()),
        };
        let receipt = sample_receipt();

        let (cid1, _) = archive_receipt(&provider, &receipt)
            .await
            .expect("archival should succeed");

        // Archiving the same receipt should produce the same CID.
        let (cid2, _) = archive_receipt(&provider, &receipt)
            .await
            .expect("archival should succeed");

        assert_eq!(cid1, cid2, "same receipt must produce same CID");
    }

    #[tokio::test]
    async fn test_archive_receipt_roundtrip() {
        let provider = MockStorageProvider {
            stored_data: std::sync::Mutex::new(HashMap::new()),
        };
        let receipt = sample_receipt();

        let (cid, _) = archive_receipt(&provider, &receipt)
            .await
            .expect("archival should succeed");

        // Retrieve the stored data.
        let stored_bytes = provider
            .retrieve(&cid)
            .await
            .expect("retrieval should succeed");

        // Deserialize and verify.
        let deserialized: ForecastVerificationReceipt =
            serde_json::from_slice(&stored_bytes).expect("deserialization should succeed");

        assert_eq!(deserialized, receipt);
    }

    #[tokio::test]
    async fn test_archive_different_receipts_different_cids() {
        let provider = MockStorageProvider {
            stored_data: std::sync::Mutex::new(HashMap::new()),
        };

        let receipt1 = sample_receipt();
        let mut receipt2 = sample_receipt();
        receipt2.commit_id = "commit-002".to_string();

        let (cid1, _) = archive_receipt(&provider, &receipt1)
            .await
            .expect("archival 1 should succeed");

        let (cid2, _) = archive_receipt(&provider, &receipt2)
            .await
            .expect("archival 2 should succeed");

        assert_ne!(cid1, cid2, "different receipts must produce different CIDs");
    }
}
