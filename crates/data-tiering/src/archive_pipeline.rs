//! Multi-cloud archive pipeline with integrity verification.
//!
//! Stores data to N providers and verifies integrity on retrieval using blake3.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wrfcoin_storage_common::error::StorageError;
use wrfcoin_storage_common::integrity::IntegrityVerifier;
use wrfcoin_storage_common::traits::StorageProvider;
use wrfcoin_storage_common::types::{ArchiveReceipt, Metadata, RetentionPolicy, StorageReceipt};

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

    /// Archive data to >= min_copies providers with integrity verification.
    ///
    /// Computes a canonical blake3 hash and stores the data to each configured
    /// provider. Returns an `ArchiveReceipt` if at least `min_copies` providers
    /// succeed. Otherwise returns a `StorageError::ProviderError`.
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
            match provider.store(data, metadata).await {
                Ok(content_id) => {
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
                                collector.record_retrieval(&provider_receipt.provider, true, elapsed);
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
    pub fn check_retention(
        &self,
        artifact_class: &str,
        age_secs: u64,
        quality_score: f64,
    ) -> bool {
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
        self.metrics.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
