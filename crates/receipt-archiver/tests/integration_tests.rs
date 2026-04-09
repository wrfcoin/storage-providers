//! Integration tests for forecast verification receipt archiver.
//!
//! These tests verify the complete workflow: serialization, archival, retrieval,
//! and round-trip verification of forecast verification receipts.

use receipt_archiver::{
    archive_receipt, canonical_json, ForecastAccuracyMetrics, ForecastVerificationReceipt,
};
use std::collections::HashMap;
use wrfcoin_storage_common::{
    types::{ContentId, HealthStatus, Metadata, StorageStats},
    StorageError, StorageProvider,
};

/// Mock storage provider for testing.
struct TestStorageProvider {
    stored_data: std::sync::Mutex<HashMap<String, Vec<u8>>>,
}

impl TestStorageProvider {
    fn new() -> Self {
        Self {
            stored_data: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl StorageProvider for TestStorageProvider {
    async fn store(&self, data: &[u8], _metadata: &Metadata) -> Result<ContentId, StorageError> {
        let cid = wrfcoin_storage_common::hashing::ipfs_content_id(data);
        self.stored_data
            .lock()
            .unwrap()
            .insert(cid.hash.clone(), data.to_vec());
        Ok(cid)
    }

    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        self.stored_data
            .lock()
            .unwrap()
            .get(&id.hash)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(id.hash.clone()))
    }

    async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
        Ok(self.stored_data.lock().unwrap().contains_key(&id.hash))
    }

    async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
        self.stored_data.lock().unwrap().remove(&id.hash);
        Ok(())
    }

    async fn health_check(&self) -> Result<HealthStatus, StorageError> {
        Ok(HealthStatus {
            healthy: true,
            provider: "test".to_string(),
            latency_ms: Some(1),
            message: Some("test provider".to_string()),
        })
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        Ok(StorageStats {
            total_stored: 0,
            total_retrieved: 0,
            total_bytes: 0,
            cache_hits: 0,
            cache_misses: 0,
        })
    }
}

fn create_test_receipt() -> ForecastVerificationReceipt {
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

/// Test 1: Canonical serialization determinism
///
/// This test verifies that the same receipt always serializes to
/// the same canonical JSON, ensuring deterministic CID computation.
#[tokio::test]
async fn test_canonical_serialization_determinism() {
    let receipt = create_test_receipt();
    let canonical1 = canonical_json(&receipt).expect("canonical JSON should succeed");
    let canonical2 = canonical_json(&receipt).expect("canonical JSON should succeed");

    assert_eq!(
        canonical1, canonical2,
        "canonical JSON must be deterministic for same receipt"
    );

    // Verify we can deserialize it back
    let deserialized: ForecastVerificationReceipt =
        serde_json::from_slice(&canonical1).expect("deserialization should succeed");
    assert_eq!(
        deserialized, receipt,
        "deserialized receipt must match original"
    );
}

/// Test 2: CID computation correctness
///
/// This test verifies that:
/// - The same receipt produces the same CID
/// - Different receipts produce different CIDs
/// - The CID format is correct (IPFS CIDv0 starting with "Qm")
#[tokio::test]
async fn test_cid_computation_correctness() {
    let provider = TestStorageProvider::new();
    let receipt = create_test_receipt();

    // Archive the receipt twice
    let (cid1, _) = archive_receipt(&provider, &receipt)
        .await
        .expect("archival should succeed");
    let (cid2, _) = archive_receipt(&provider, &receipt)
        .await
        .expect("archival should succeed");

    // Same receipt must produce same CID
    assert_eq!(cid1, cid2, "same receipt must produce same CID");

    // Verify CID format
    assert_eq!(cid1.provider, "ipfs", "provider must be ipfs");
    assert!(
        cid1.hash.starts_with("Qm"),
        "IPFS CID must start with Qm, got: {}",
        cid1.hash
    );

    // Different receipt must produce different CID
    let mut receipt2 = create_test_receipt();
    receipt2.commit_id = "commit-002".to_string();
    let (cid3, _) = archive_receipt(&provider, &receipt2)
        .await
        .expect("archival should succeed");

    assert_ne!(
        cid1, cid3,
        "different receipts must produce different CIDs"
    );
}

/// Test 3: Round-trip archive and retrieve
///
/// This test verifies the complete workflow:
/// 1. Archive a receipt to storage
/// 2. Retrieve it by CID
/// 3. Deserialize and verify it matches the original
#[tokio::test]
async fn test_roundtrip_archive_and_retrieve() {
    let provider = TestStorageProvider::new();
    let original_receipt = create_test_receipt();

    // Archive the receipt
    let (cid, _) = archive_receipt(&provider, &original_receipt)
        .await
        .expect("archival should succeed");

    // Verify the receipt exists
    let exists = provider
        .exists(&cid)
        .await
        .expect("existence check should succeed");
    assert!(exists, "archived receipt should exist in storage");

    // Retrieve the receipt
    let stored_bytes = provider
        .retrieve(&cid)
        .await
        .expect("retrieval should succeed");

    // Deserialize and verify
    let retrieved_receipt: ForecastVerificationReceipt =
        serde_json::from_slice(&stored_bytes).expect("deserialization should succeed");
    assert_eq!(
        retrieved_receipt, original_receipt,
        "retrieved receipt must match original"
    );
}

/// Test 4: Multiple receipts with different manifests
///
/// This test verifies that receipts with different manifest digests
/// produce different CIDs, even if other fields are similar.
#[tokio::test]
async fn test_different_manifests_produce_different_cids() {
    let provider = TestStorageProvider::new();

    let receipt1 = ForecastVerificationReceipt::new(
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
        "nonce-1".to_string(),
    );

    let receipt2 = ForecastVerificationReceipt::new(
        "c".repeat(64),  // Different manifest digest
        "commit-001".to_string(),
        "b".repeat(64),
        ForecastAccuracyMetrics {
            mae: 0.5,
            rmse: 0.7,
            bias: -0.1,
            skill_score: 0.85,
        },
        1_000_000,
        "nonce-1".to_string(),
    );

    let (cid1, _) = archive_receipt(&provider, &receipt1)
        .await
        .expect("archival 1 should succeed");
    let (cid2, _) = archive_receipt(&provider, &receipt2)
        .await
        .expect("archival 2 should succeed");

    assert_ne!(
        cid1, cid2,
        "receipts with different manifest digests must have different CIDs"
    );
}

/// Test 5: Signature and proof preservation
///
/// This test verifies that optional signature and ZK proof fields
/// are properly serialized and preserved through the archival cycle.
#[tokio::test]
async fn test_signature_and_proof_preservation() {
    let provider = TestStorageProvider::new();

    let receipt = create_test_receipt()
        .with_signature("sig-abc123".to_string())
        .with_zk_proof("proof-xyz789".to_string());

    // Archive
    let (cid, _) = archive_receipt(&provider, &receipt)
        .await
        .expect("archival should succeed");

    // Retrieve
    let stored_bytes = provider
        .retrieve(&cid)
        .await
        .expect("retrieval should succeed");

    let retrieved: ForecastVerificationReceipt =
        serde_json::from_slice(&stored_bytes).expect("deserialization should succeed");

    assert_eq!(
        retrieved.signature,
        Some("sig-abc123".to_string()),
        "signature must be preserved"
    );
    assert_eq!(
        retrieved.zk_proof,
        Some("proof-xyz789".to_string()),
        "ZK proof must be preserved"
    );
}
