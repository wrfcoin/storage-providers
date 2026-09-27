//! Multi-cloud archive pipeline with integrity verification.
//!
//! Stores data to N providers and verifies integrity on retrieval using blake3.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wrfcoin_storage_common::deduplication::DeduplicationRegistry;
use wrfcoin_storage_common::error::StorageError;
use wrfcoin_storage_common::integrity::IntegrityVerifier;
use wrfcoin_storage_common::provenance::{ProvenancePolicy, ProvenanceValidator};
use wrfcoin_storage_common::traits::StorageProvider;
use wrfcoin_storage_common::types::{
    AdmissionDecision, ArchiveReceipt, Metadata, ProvenanceChain, QuarantineRecord,
    RetentionPolicy, StorageReceipt,
};

use crate::metrics::MetricsCollector;

/// Returns current unix timestamp in seconds.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Metrics counters for the archive pipeline.
#[derive(Debug, Clone, Default)]
pub struct ArchiveMetrics {
    pub archive_attempts: u64,
    pub archive_successes: u64,
    pub archive_failures: u64,
    pub retrieval_attempts: u64,
    pub retrieval_successes: u64,
    pub retrieval_failures: u64,
    pub integrity_checks: u64,
    pub integrity_failures: u64,
    pub retrieval_latency_sum_ms: u64,
    pub retrieval_count: u64,
}

impl ArchiveMetrics {
    /// Create zeroed metrics.
    pub fn new() -> Self {
        Self {
            archive_attempts: 0,
            archive_successes: 0,
            archive_failures: 0,
            retrieval_attempts: 0,
            retrieval_successes: 0,
            retrieval_failures: 0,
            integrity_checks: 0,
            integrity_failures: 0,
            retrieval_latency_sum_ms: 0,
            retrieval_count: 0,
        }
    }

    /// Average retrieval latency in milliseconds. Returns 0.0 if no retrievals.
    pub fn avg_retrieval_latency_ms(&self) -> f64 {
        if self.retrieval_count == 0 {
            return 0.0;
        }
        self.retrieval_latency_sum_ms as f64 / self.retrieval_count as f64
    }
}

/// Archive pipeline that stores data to N providers with integrity verification.
pub struct ArchivePipeline {
    providers: Vec<(String, Arc<dyn StorageProvider>)>,
    min_copies: usize,
    retention_policies: Vec<RetentionPolicy>,
    metrics: std::sync::Mutex<ArchiveMetrics>,
    metrics_collector: Option<Arc<dyn MetricsCollector>>,
    provenance_validator: Option<ProvenanceValidator>,
    quarantine_log: std::sync::Mutex<Vec<QuarantineRecord>>,
    dedup_registry: Arc<DeduplicationRegistry>,
}

impl ArchivePipeline {
    /// Create a new archive pipeline requiring at least `min_copies` successful stores.
    pub fn new(min_copies: usize) -> Self {
        Self {
            providers: Vec::new(),
            min_copies,
            retention_policies: Vec::new(),
            metrics: std::sync::Mutex::new(ArchiveMetrics::new()),
            metrics_collector: None,
            provenance_validator: None,
            quarantine_log: std::sync::Mutex::new(Vec::new()),
            dedup_registry: Arc::new(DeduplicationRegistry::new()),
        }
    }

    /// Add a named storage provider to the pipeline.
    pub fn add_provider(&mut self, name: &str, provider: Arc<dyn StorageProvider>) {
        self.providers.push((name.to_string(), provider));
    }

    /// Add a retention policy.
    pub fn add_retention_policy(&mut self, policy: RetentionPolicy) {
        self.retention_policies.push(policy);
    }

    /// Set an external metrics collector.
    pub fn set_metrics_collector(&mut self, collector: Arc<dyn MetricsCollector>) {
        self.metrics_collector = Some(collector);
    }

    /// Get access to the deduplication registry.
    pub fn dedup_registry(&self) -> Arc<DeduplicationRegistry> {
        Arc::clone(&self.dedup_registry)
    }

    /// Archive data to >= min_copies providers with integrity verification.
    ///
    /// Computes a canonical blake3 hash and stores the data to each configured
    /// provider. Checks the deduplication registry first — if a provider already has
    /// a copy, skips the store and uses the cached ContentId.
    ///
    /// Returns an `ArchiveReceipt` if at least `min_copies` providers have a copy
    /// (either newly stored or cached). Otherwise returns a `StorageError::ProviderError`.
    pub async fn archive(
        &self,
        data: &[u8],
        metadata: &Metadata,
    ) -> Result<ArchiveReceipt, StorageError> {
        let canonical_hash = IntegrityVerifier::hash(data);
        let timestamp = now();
        let mut receipts = Vec::new();
        let mut errors = Vec::new();

        for (name, provider) in &self.providers {
            {
                let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
                m.archive_attempts += 1;
            }

            // Check if this provider already has a copy via deduplication registry
            let existing_content_id = self
                .dedup_registry
                .lookup(&canonical_hash, name)
                .unwrap_or(None);

            let result = if let Some(content_id) = existing_content_id {
                // Provider already has this content — skip store and use cached copy
                Ok((content_id, true)) // true = was cached
            } else {
                // Provider doesn't have it — store it
                match provider.store(data, metadata).await {
                    Ok(content_id) => Ok((content_id, false)), // false = was not cached
                    Err(e) => Err(e),
                }
            };

            match result {
                Ok((content_id, was_cached)) => {
                    // Only add to registry if this was a new store (not a cache hit)
                    if !was_cached {
                        let _ = self.dedup_registry.insert(
                            canonical_hash.clone(),
                            data.len() as u64,
                            content_id.clone(),
                            timestamp,
                        );
                    }

                    receipts.push(StorageReceipt {
                        content_id,
                        provider: name.clone(),
                        timestamp,
                        size: data.len() as u64,
                    });
                    let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
                    m.archive_successes += 1;
                    if let Some(ref collector) = self.metrics_collector {
                        collector.record_archive_attempt(name, true);
                    }
                }
                Err(e) => {
                    let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
                    m.archive_failures += 1;
                    if let Some(ref collector) = self.metrics_collector {
                        collector.record_archive_attempt(name, false);
                    }
                    errors.push(e);
                }
            }
        }

        if receipts.len() < self.min_copies {
            return Err(StorageError::ProviderError(format!(
                "Only {} of {} required copies stored",
                receipts.len(),
                self.min_copies
            )));
        }

        Ok(ArchiveReceipt {
            canonical_cid: canonical_hash,
            provider_receipts: receipts,
            archived_at: timestamp,
            providers_attempted: self.providers.len(),
            providers_succeeded: self.providers.len() - errors.len(),
            provenance_result: None,
            admission_decision: None,
        })
    }

    /// Retrieve data with integrity verification — tries providers in order.
    ///
    /// Walks through the providers listed in the receipt, retrieves the data,
    /// and verifies its blake3 hash matches the canonical CID. Returns the
    /// first valid copy, or `StorageError::NotFound` if all fail.
    pub async fn retrieve_verified(
        &self,
        receipt: &ArchiveReceipt,
    ) -> Result<Vec<u8>, StorageError> {
        let start = std::time::Instant::now();

        {
            let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
            m.retrieval_attempts += 1;
        }

        for provider_receipt in &receipt.provider_receipts {
            if let Some((_, provider)) = self
                .providers
                .iter()
                .find(|(n, _)| *n == provider_receipt.provider)
            {
                match provider.retrieve(&provider_receipt.content_id).await {
                    Ok(data) => {
                        let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
                        m.integrity_checks += 1;

                        let passed = IntegrityVerifier::verify(&data, &receipt.canonical_cid);
                        if let Some(ref collector) = self.metrics_collector {
                            collector.record_integrity_check(&provider_receipt.provider, passed);
                        }

                        if passed {
                            let elapsed = start.elapsed().as_millis() as u64;
                            m.retrieval_successes += 1;
                            m.retrieval_latency_sum_ms += elapsed;
                            m.retrieval_count += 1;
                            if let Some(ref collector) = self.metrics_collector {
                                collector.record_retrieval(
                                    &provider_receipt.provider,
                                    true,
                                    elapsed,
                                );
                            }
                            return Ok(data);
                        } else {
                            m.integrity_failures += 1;
                            // Try next provider
                        }
                    }
                    Err(_) => continue,
                }
            }
        }

        {
            let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
            m.retrieval_failures += 1;
        }
        if let Some(ref collector) = self.metrics_collector {
            let elapsed = start.elapsed().as_millis() as u64;
            collector.record_retrieval("none", false, elapsed);
        }

        Err(StorageError::NotFound(
            "No provider returned valid data".into(),
        ))
    }

    /// Check retention policy for given artifact.
    ///
    /// Returns `true` if data should be retained. If no policies match the
    /// artifact class, retention defaults to `true` (keep).
    pub fn check_retention(&self, artifact_class: &str, age_secs: u64, quality_score: f64) -> bool {
        let matching: Vec<_> = self
            .retention_policies
            .iter()
            .filter(|p| p.artifact_class == artifact_class)
            .collect();

        if matching.is_empty() {
            return true; // No policy = retain by default
        }

        matching
            .iter()
            .any(|p| p.should_retain(age_secs, quality_score))
    }

    /// Get a snapshot of current metrics.
    pub fn metrics(&self) -> ArchiveMetrics {
        self.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Set a provenance policy, enabling provenance-gated archiving.
    pub fn set_provenance_policy(&mut self, policy: ProvenancePolicy) {
        self.provenance_validator = Some(ProvenanceValidator::new(policy));
    }

    /// Archive data with provenance admission control.
    ///
    /// If no provenance validator is set, falls through to `archive()` and
    /// returns `(Accepted, Some(receipt))`.
    ///
    /// Otherwise:
    /// - **Accepted**: stores normally, populates audit fields on receipt.
    /// - **Quarantined**: creates a quarantine record, still stores (flagged),
    ///   populates audit fields.
    /// - **Rejected**: returns `(Rejected, None)` — data is not stored.
    pub async fn archive_with_provenance(
        &self,
        data: &[u8],
        metadata: &Metadata,
        provenance: Option<&ProvenanceChain>,
    ) -> Result<(AdmissionDecision, Option<ArchiveReceipt>), StorageError> {
        let validator = match &self.provenance_validator {
            Some(v) => v,
            None => {
                let receipt = self.archive(data, metadata).await?;
                return Ok((AdmissionDecision::Accepted, Some(receipt)));
            }
        };

        let (decision, reason) = validator.decide_admission(provenance);

        match decision {
            AdmissionDecision::Accepted => {
                let validation_result = provenance.map(|c| validator.validate_chain(c));
                let mut receipt = self.archive(data, metadata).await?;
                receipt.provenance_result = validation_result;
                receipt.admission_decision = Some(AdmissionDecision::Accepted);
                Ok((AdmissionDecision::Accepted, Some(receipt)))
            }
            AdmissionDecision::Quarantined => {
                let validation_result = provenance.map(|c| validator.validate_chain(c));
                let now_ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let quarantine_id = format!(
                    "q-{}-{}",
                    now_ts,
                    provenance.map(|c| c.chain_id.as_str()).unwrap_or("unknown")
                );
                let record = QuarantineRecord {
                    quarantine_id,
                    reason: reason.unwrap_or_else(|| "unknown".to_string()),
                    detected_at: now_ts,
                    review_required: true,
                    reinstated: false,
                };
                {
                    let mut log = self
                        .quarantine_log
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    log.push(record);
                }
                // Still store, but flagged
                let mut receipt = self.archive(data, metadata).await?;
                receipt.provenance_result = validation_result;
                receipt.admission_decision = Some(AdmissionDecision::Quarantined);
                Ok((AdmissionDecision::Quarantined, Some(receipt)))
            }
            AdmissionDecision::Rejected => Ok((AdmissionDecision::Rejected, None)),
        }
    }

    /// Return a snapshot of all quarantine records.
    pub fn quarantine_records(&self) -> Vec<QuarantineRecord> {
        self.quarantine_log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wrfcoin_storage_common::types::ContentId;

    /// Mock storage provider for testing.
    struct MockStorageProvider {
        provider_name: String,
        data: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
        store_call_count: std::sync::atomic::AtomicU64,
    }

    impl MockStorageProvider {
        fn new(provider_name: &str) -> Self {
            Self {
                provider_name: provider_name.to_string(),
                data: std::sync::Mutex::new(std::collections::HashMap::new()),
                store_call_count: std::sync::atomic::AtomicU64::new(0),
            }
        }

        fn store_call_count(&self) -> u64 {
            self.store_call_count
                .load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    #[async_trait::async_trait]
    impl StorageProvider for MockStorageProvider {
        async fn store(
            &self,
            data: &[u8],
            _metadata: &Metadata,
        ) -> Result<ContentId, StorageError> {
            self.store_call_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let ipfs_cid = wrfcoin_storage_common::ipfs_content_id(data);
            self.data
                .lock()
                .unwrap()
                .insert(ipfs_cid.hash.clone(), data.to_vec());
            // Return a ContentId with this provider's name but the same hash
            Ok(ContentId {
                provider: self.provider_name.clone(),
                hash: ipfs_cid.hash,
            })
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

        async fn stats(&self) -> Result<wrfcoin_storage_common::types::StorageStats, StorageError> {
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
    async fn test_archive_same_data_twice_skips_second_store() {
        let mut pipeline = ArchivePipeline::new(1);
        let mock_provider = Arc::new(MockStorageProvider::new("ipfs"));
        pipeline.add_provider("ipfs", mock_provider.clone() as Arc<dyn StorageProvider>);

        let test_data = b"test weather data";
        let metadata = Metadata {
            content_type: "application/octet-stream".to_string(),
            size: test_data.len() as u64,
            timestamp: 1000,
            tags: std::collections::HashMap::new(),
        };

        // First archive call
        let result1 = pipeline.archive(test_data, &metadata).await;
        assert!(result1.is_ok());
        assert_eq!(mock_provider.store_call_count(), 1);

        // Second archive call with same data
        let result2 = pipeline.archive(test_data, &metadata).await;
        assert!(result2.is_ok());

        // Second call should NOT have called store again (dedup)
        assert_eq!(mock_provider.store_call_count(), 1);

        // Both receipts should have the same ContentId
        assert_eq!(
            result1.unwrap().provider_receipts[0].content_id,
            result2.unwrap().provider_receipts[0].content_id
        );
    }

    #[tokio::test]
    async fn test_archive_different_data_stores_separately() {
        let mut pipeline = ArchivePipeline::new(1);
        let mock_provider = Arc::new(MockStorageProvider::new("ipfs"));
        pipeline.add_provider("ipfs", mock_provider.clone() as Arc<dyn StorageProvider>);

        let data1 = b"test data 1";
        let data2 = b"test data 2";
        let metadata = Metadata {
            content_type: "application/octet-stream".to_string(),
            size: 11,
            timestamp: 1000,
            tags: std::collections::HashMap::new(),
        };

        let result1 = pipeline.archive(data1, &metadata).await;
        assert!(result1.is_ok());
        assert_eq!(mock_provider.store_call_count(), 1);

        let result2 = pipeline.archive(data2, &metadata).await;
        assert!(result2.is_ok());
        assert_eq!(mock_provider.store_call_count(), 2);

        // Different data should produce different ContentIds
        assert_ne!(
            result1.unwrap().provider_receipts[0].content_id,
            result2.unwrap().provider_receipts[0].content_id
        );
    }

    #[tokio::test]
    async fn test_archive_with_multiple_providers_deduplicates() {
        let mut pipeline = ArchivePipeline::new(2);
        let ipfs_provider = Arc::new(MockStorageProvider::new("ipfs"));
        let filecoin_provider = Arc::new(MockStorageProvider::new("filecoin"));
        pipeline.add_provider("ipfs", ipfs_provider.clone() as Arc<dyn StorageProvider>);
        pipeline.add_provider(
            "filecoin",
            filecoin_provider.clone() as Arc<dyn StorageProvider>,
        );

        let test_data = b"weather observation data";
        let metadata = Metadata {
            content_type: "application/json".to_string(),
            size: test_data.len() as u64,
            timestamp: 1000,
            tags: std::collections::HashMap::new(),
        };

        // First archive — both providers should store
        let result1 = pipeline.archive(test_data, &metadata).await;
        assert!(result1.is_ok());
        assert_eq!(ipfs_provider.store_call_count(), 1);
        assert_eq!(filecoin_provider.store_call_count(), 1);

        // Second archive with same data — neither should store
        let result2 = pipeline.archive(test_data, &metadata).await;
        assert!(result2.is_ok());
        assert_eq!(ipfs_provider.store_call_count(), 1);
        assert_eq!(filecoin_provider.store_call_count(), 1);

        // All four receipts (2 from each call, 2 providers) should reference the same blobs
        let r1 = result1.unwrap();
        let r2 = result2.unwrap();
        assert_eq!(r1.provider_receipts.len(), 2);
        assert_eq!(r2.provider_receipts.len(), 2);
    }

    #[tokio::test]
    async fn test_dedup_registry_can_be_accessed() {
        let pipeline = ArchivePipeline::new(1);
        let registry = pipeline.dedup_registry();

        // Registry should be empty initially
        assert!(registry.is_empty().unwrap());

        let test_data = b"test";
        let metadata = Metadata {
            content_type: "application/octet-stream".to_string(),
            size: 4,
            timestamp: 1000,
            tags: std::collections::HashMap::new(),
        };

        // Create a provider and add it
        let mock_provider = Arc::new(MockStorageProvider::new("ipfs"));
        let mut pipeline = ArchivePipeline::new(1);
        pipeline.add_provider("ipfs", mock_provider);
        let registry = pipeline.dedup_registry();

        // Archive some data
        let _ = pipeline.archive(test_data, &metadata).await;

        // Registry should now have an entry
        assert!(!registry.is_empty().unwrap());
        assert!(registry.len().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_archive_preserves_data_through_dedup() {
        let mut pipeline = ArchivePipeline::new(1);
        let mock_provider = Arc::new(MockStorageProvider::new("ipfs"));
        pipeline.add_provider("ipfs", mock_provider.clone() as Arc<dyn StorageProvider>);

        let test_data = b"important weather station data";
        let metadata = Metadata {
            content_type: "application/json".to_string(),
            size: test_data.len() as u64,
            timestamp: 1000,
            tags: std::collections::HashMap::new(),
        };

        // Archive data twice
        let result1 = pipeline.archive(test_data, &metadata).await;
        assert!(result1.is_ok());

        let result2 = pipeline.archive(test_data, &metadata).await;
        assert!(result2.is_ok());

        // Retrieve from both and verify
        let receipt1 = result1.unwrap();
        let content_id1 = &receipt1.provider_receipts[0].content_id;

        let retrieved = mock_provider.retrieve(content_id1).await;
        assert!(retrieved.is_ok());
        assert_eq!(retrieved.unwrap(), test_data);
    }
}
