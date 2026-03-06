//! Integration tests for the multi-cloud archive pipeline.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wrfcoin_data_tiering::archive_pipeline::ArchivePipeline;
use wrfcoin_data_tiering::retention::RetentionManager;
use wrfcoin_storage_common::error::StorageError;
use wrfcoin_storage_common::integrity::IntegrityVerifier;
use wrfcoin_storage_common::provenance::ProvenancePolicy;
use wrfcoin_storage_common::traits::StorageProvider;
use wrfcoin_storage_common::types::*;

// ---------------------------------------------------------------------------
// Mock providers
// ---------------------------------------------------------------------------

/// Mock provider that stores data in memory.
struct MockProvider {
    name: String,
    store: Mutex<HashMap<String, Vec<u8>>>,
    should_fail: Mutex<bool>,
}

impl MockProvider {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            store: Mutex::new(HashMap::new()),
            should_fail: Mutex::new(false),
        }
    }

    fn set_fail(&self, fail: bool) {
        *self.should_fail.lock().unwrap() = fail;
    }
}

#[async_trait]
impl StorageProvider for MockProvider {
    async fn store(&self, data: &[u8], _metadata: &Metadata) -> Result<ContentId, StorageError> {
        if *self.should_fail.lock().unwrap() {
            return Err(StorageError::ConnectionFailed("mock failure".into()));
        }
        let hash = IntegrityVerifier::hash(data);
        self.store
            .lock()
            .unwrap()
            .insert(hash.clone(), data.to_vec());
        Ok(ContentId {
            provider: self.name.clone(),
            hash,
        })
    }

    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        self.store
            .lock()
            .unwrap()
            .get(&id.hash)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(id.hash.clone()))
    }

    async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
        Ok(self.store.lock().unwrap().contains_key(&id.hash))
    }

    async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
        self.store.lock().unwrap().remove(&id.hash);
        Ok(())
    }

    async fn health_check(&self) -> Result<HealthStatus, StorageError> {
        Ok(HealthStatus {
            healthy: true,
            provider: self.name.clone(),
            latency_ms: Some(1),
            message: None,
        })
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        Ok(StorageStats::default())
    }
}

/// Mock provider that corrupts data on retrieve.
struct CorruptingProvider {
    inner: MockProvider,
}

impl CorruptingProvider {
    fn new(name: &str) -> Self {
        Self {
            inner: MockProvider::new(name),
        }
    }
}

#[async_trait]
impl StorageProvider for CorruptingProvider {
    async fn store(&self, data: &[u8], metadata: &Metadata) -> Result<ContentId, StorageError> {
        self.inner.store(data, metadata).await
    }

    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        let mut data = self.inner.retrieve(id).await?;
        if !data.is_empty() {
            data[0] ^= 0xFF; // flip first byte
        }
        Ok(data)
    }

    async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
        self.inner.exists(id).await
    }

    async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
        self.inner.delete(id).await
    }

    async fn health_check(&self) -> Result<HealthStatus, StorageError> {
        self.inner.health_check().await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        self.inner.stats().await
    }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn test_metadata() -> Metadata {
    Metadata {
        content_type: "application/octet-stream".to_string(),
        size: 0,
        timestamp: 1_000_000,
        tags: HashMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_archive_to_multiple_providers() {
    let mut pipeline = ArchivePipeline::new(2);
    pipeline.add_provider("ipfs", Arc::new(MockProvider::new("ipfs")));
    pipeline.add_provider("filecoin", Arc::new(MockProvider::new("filecoin")));
    pipeline.add_provider("arweave", Arc::new(MockProvider::new("arweave")));

    let data = b"weather observation data 2026-03-04";
    let metadata = test_metadata();

    let receipt = pipeline.archive(data, &metadata).await.unwrap();

    assert_eq!(receipt.provider_receipts.len(), 3);
    assert_eq!(receipt.providers_attempted, 3);
    assert_eq!(receipt.providers_succeeded, 3);
    assert_eq!(receipt.canonical_cid, IntegrityVerifier::hash(data));
}

#[tokio::test]
async fn test_archive_fails_below_min_copies() {
    let p1 = Arc::new(MockProvider::new("ipfs"));
    let p2 = Arc::new(MockProvider::new("filecoin"));
    let p3 = Arc::new(MockProvider::new("arweave"));

    // Set 2 of 3 providers to fail
    p2.set_fail(true);
    p3.set_fail(true);

    let mut pipeline = ArchivePipeline::new(3);
    pipeline.add_provider("ipfs", p1);
    pipeline.add_provider("filecoin", p2);
    pipeline.add_provider("arweave", p3);

    let data = b"some data";
    let metadata = test_metadata();

    let result = pipeline.archive(data, &metadata).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    match err {
        StorageError::ProviderError(msg) => {
            assert!(msg.contains("1"), "expected '1' in error: {}", msg);
            assert!(msg.contains("3"), "expected '3' in error: {}", msg);
        }
        other => panic!("expected ProviderError, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_retrieve_verified_success() {
    let mut pipeline = ArchivePipeline::new(1);
    pipeline.add_provider("ipfs", Arc::new(MockProvider::new("ipfs")));

    let data = b"forecast model output";
    let metadata = test_metadata();

    let receipt = pipeline.archive(data, &metadata).await.unwrap();
    let retrieved = pipeline.retrieve_verified(&receipt).await.unwrap();

    assert_eq!(retrieved, data);
}

#[tokio::test]
async fn test_retrieve_rejects_corrupted_data() {
    // Corrupting provider first, then a good provider
    let mut pipeline = ArchivePipeline::new(1);
    pipeline.add_provider("corrupt", Arc::new(CorruptingProvider::new("corrupt")));
    pipeline.add_provider("good", Arc::new(MockProvider::new("good")));

    let data = b"important weather data that must not be tampered with";
    let metadata = test_metadata();

    let receipt = pipeline.archive(data, &metadata).await.unwrap();
    assert_eq!(receipt.provider_receipts.len(), 2);

    // Retrieve should skip corrupt provider and succeed from good provider
    let retrieved = pipeline.retrieve_verified(&receipt).await.unwrap();
    assert_eq!(retrieved, data);

    // Verify integrity failure was tracked
    let metrics = pipeline.metrics();
    assert!(
        metrics.integrity_failures >= 1,
        "expected at least 1 integrity failure, got {}",
        metrics.integrity_failures
    );
}

#[tokio::test]
async fn test_retrieve_fails_all_corrupted() {
    let mut pipeline = ArchivePipeline::new(1);
    pipeline.add_provider("corrupt1", Arc::new(CorruptingProvider::new("corrupt1")));
    pipeline.add_provider("corrupt2", Arc::new(CorruptingProvider::new("corrupt2")));

    let data = b"data that will be corrupted everywhere";
    let metadata = test_metadata();

    let receipt = pipeline.archive(data, &metadata).await.unwrap();
    let result = pipeline.retrieve_verified(&receipt).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        StorageError::NotFound(msg) => {
            assert!(msg.contains("No provider"), "unexpected message: {}", msg);
        }
        other => panic!("expected NotFound, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_retention_policy_check() {
    let mut pipeline = ArchivePipeline::new(1);

    let mut policy = RetentionPolicy::new("forecast", DataTier::Archive, 2);
    policy.max_age_secs = 3600; // 1 hour
    pipeline.add_retention_policy(policy);

    // Young data should be retained
    assert!(pipeline.check_retention("forecast", 1800, 0.9));

    // Old data should not be retained
    assert!(!pipeline.check_retention("forecast", 7200, 0.9));
}

#[tokio::test]
async fn test_retention_policy_quality_gate() {
    let mut pipeline = ArchivePipeline::new(1);

    let mut policy = RetentionPolicy::new("observation", DataTier::Cold, 1);
    policy.min_quality_score = 0.5;
    pipeline.add_retention_policy(policy);

    // High quality — retain
    assert!(pipeline.check_retention("observation", 100, 0.8));

    // Low quality — do not retain
    assert!(!pipeline.check_retention("observation", 100, 0.3));
}

#[tokio::test]
async fn test_metrics_tracking() {
    let mut pipeline = ArchivePipeline::new(1);
    pipeline.add_provider("ipfs", Arc::new(MockProvider::new("ipfs")));

    let data = b"metrics test data";
    let metadata = test_metadata();

    let receipt = pipeline.archive(data, &metadata).await.unwrap();
    let _retrieved = pipeline.retrieve_verified(&receipt).await.unwrap();

    let metrics = pipeline.metrics();
    assert_eq!(metrics.archive_attempts, 1);
    assert_eq!(metrics.archive_successes, 1);
    assert_eq!(metrics.archive_failures, 0);
    assert_eq!(metrics.retrieval_attempts, 1);
    assert_eq!(metrics.retrieval_successes, 1);
    assert_eq!(metrics.retrieval_failures, 0);
    assert_eq!(metrics.integrity_checks, 1);
    assert_eq!(metrics.integrity_failures, 0);
    assert_eq!(metrics.retrieval_count, 1);
}

#[tokio::test]
async fn test_retention_manager_standalone() {
    let mut mgr = RetentionManager::new();

    let mut policy = RetentionPolicy::new("forecast", DataTier::Archive, 3);
    policy.max_age_secs = 86400;
    policy.min_quality_score = 0.4;
    mgr.add_policy(policy);

    assert_eq!(mgr.min_copies_for("forecast"), 3);
    assert_eq!(mgr.min_copies_for("unknown"), 1);

    assert!(mgr.should_retain("forecast", &DataTier::Archive, 3600, 0.9));
    assert!(!mgr.should_retain("forecast", &DataTier::Archive, 100_000, 0.9));
    assert!(!mgr.should_retain("forecast", &DataTier::Archive, 100, 0.2));

    // Unmatched tier defaults to retain
    assert!(mgr.should_retain("forecast", &DataTier::Hot, 999999, 0.0));
}

#[tokio::test]
async fn test_archive_metrics_avg_latency() {
    use wrfcoin_data_tiering::archive_pipeline::ArchiveMetrics;

    let mut m = ArchiveMetrics::new();
    assert_eq!(m.avg_retrieval_latency_ms(), 0.0);

    m.retrieval_latency_sum_ms = 100;
    m.retrieval_count = 4;
    assert!((m.avg_retrieval_latency_ms() - 25.0).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------------------
// Provenance integration tests
// ---------------------------------------------------------------------------

fn valid_provenance_chain() -> ProvenanceChain {
    ProvenanceChain {
        chain_id: "chain-1".to_string(),
        root_hash: "root".to_string(),
        links: vec![
            ProvenanceLink {
                artifact_hash: "root".to_string(),
                parent_hash: None,
                signer: "signer-1".to_string(),
                signature: "sig-1".to_string(),
                algorithm: "ed25519".to_string(),
                signed_at: 10,
            },
            ProvenanceLink {
                artifact_hash: "child-1".to_string(),
                parent_hash: Some("root".to_string()),
                signer: "signer-2".to_string(),
                signature: "sig-2".to_string(),
                algorithm: "ed25519".to_string(),
                signed_at: 11,
            },
        ],
    }
}

#[tokio::test]
async fn test_archive_with_provenance_valid() {
    let mut pipeline = ArchivePipeline::new(1);
    pipeline.add_provider("ipfs", Arc::new(MockProvider::new("ipfs")));
    pipeline.set_provenance_policy(ProvenancePolicy::default());

    let data = b"provenance-gated weather data";
    let metadata = test_metadata();
    let chain = valid_provenance_chain();

    let (decision, receipt) = pipeline
        .archive_with_provenance(data, &metadata, Some(&chain))
        .await
        .unwrap();

    assert_eq!(decision, AdmissionDecision::Accepted);
    let receipt = receipt.expect("receipt should be present for accepted data");
    assert_eq!(receipt.admission_decision, Some(AdmissionDecision::Accepted));
    assert!(receipt.provenance_result.is_some());
    assert!(receipt.provenance_result.unwrap().valid);
}

#[tokio::test]
async fn test_archive_with_provenance_missing_rejected() {
    let mut pipeline = ArchivePipeline::new(1);
    pipeline.add_provider("ipfs", Arc::new(MockProvider::new("ipfs")));
    pipeline.set_provenance_policy(ProvenancePolicy {
        require_provenance: true,
        min_chain_length: 1,
        allowed_algorithms: Vec::new(),
    });

    let data = b"data without provenance";
    let metadata = test_metadata();

    let (decision, receipt) = pipeline
        .archive_with_provenance(data, &metadata, None)
        .await
        .unwrap();

    assert_eq!(decision, AdmissionDecision::Rejected);
    assert!(receipt.is_none());
}

#[tokio::test]
async fn test_archive_with_provenance_invalid_quarantined() {
    let mut pipeline = ArchivePipeline::new(1);
    pipeline.add_provider("ipfs", Arc::new(MockProvider::new("ipfs")));
    pipeline.set_provenance_policy(ProvenancePolicy::default());

    let bad_chain = ProvenanceChain {
        chain_id: "bad-chain".to_string(),
        root_hash: "root".to_string(),
        links: vec![ProvenanceLink {
            artifact_hash: "wrong-hash".to_string(),
            parent_hash: None,
            signer: "signer".to_string(),
            signature: "sig".to_string(),
            algorithm: "ed25519".to_string(),
            signed_at: 1,
        }],
    };

    let data = b"bad provenance data";
    let metadata = test_metadata();

    let (decision, receipt) = pipeline
        .archive_with_provenance(data, &metadata, Some(&bad_chain))
        .await
        .unwrap();

    assert_eq!(decision, AdmissionDecision::Quarantined);
    let receipt = receipt.expect("quarantined data should still be stored");
    assert_eq!(receipt.admission_decision, Some(AdmissionDecision::Quarantined));
}

#[tokio::test]
async fn test_archive_without_validator_bypasses() {
    let mut pipeline = ArchivePipeline::new(1);
    pipeline.add_provider("ipfs", Arc::new(MockProvider::new("ipfs")));
    // No provenance policy set — should bypass provenance checks

    let data = b"unvalidated data";
    let metadata = test_metadata();

    let (decision, receipt) = pipeline
        .archive_with_provenance(data, &metadata, None)
        .await
        .unwrap();

    assert_eq!(decision, AdmissionDecision::Accepted);
    let receipt = receipt.expect("should get receipt when no validator is set");
    // Audit fields should be None when no validator is active
    assert!(receipt.provenance_result.is_none());
    assert!(receipt.admission_decision.is_none());
}

#[tokio::test]
async fn test_quarantine_records_accessible() {
    let mut pipeline = ArchivePipeline::new(1);
    pipeline.add_provider("ipfs", Arc::new(MockProvider::new("ipfs")));
    pipeline.set_provenance_policy(ProvenancePolicy::default());

    // Initially empty
    assert!(pipeline.quarantine_records().is_empty());

    // Submit data with invalid provenance
    let bad_chain = ProvenanceChain {
        chain_id: "bad-chain".to_string(),
        root_hash: "root".to_string(),
        links: vec![ProvenanceLink {
            artifact_hash: "wrong".to_string(),
            parent_hash: None,
            signer: "signer".to_string(),
            signature: "sig".to_string(),
            algorithm: "ed25519".to_string(),
            signed_at: 1,
        }],
    };

    let _ = pipeline
        .archive_with_provenance(b"bad data", &test_metadata(), Some(&bad_chain))
        .await
        .unwrap();

    let records = pipeline.quarantine_records();
    assert_eq!(records.len(), 1);
    assert!(records[0].review_required);
    assert!(!records[0].reinstated);
    assert!(!records[0].reason.is_empty());
}
