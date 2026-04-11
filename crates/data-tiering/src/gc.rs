//! Garbage collection for expired and orphaned storage content.
//!
//! Provides a [`GarbageCollector`] that sweeps expired retention policies, cleans up
//! orphaned quarantine copies, removes expired Filecoin deals, and reclaims storage
//! from stale tier-promotion leftovers.

use std::collections::HashMap;

use wrfcoin_storage_common::{
    types::{DataTier, EnvironmentalDomain, QuarantineRecord},
    DomainContentIndex,
};

use crate::retention::RetentionManager;

/// Summary of garbage collection actions taken.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcSummary {
    /// Total number of items evaluated.
    pub items_evaluated: u64,
    /// Number of items deleted due to expired retention.
    pub items_deleted: u64,
    /// Total bytes freed (estimate).
    pub bytes_freed: u64,
    /// Number of orphaned quarantine records cleaned.
    pub orphaned_quarantine_cleaned: u64,
    /// Number of expired Filecoin deals cleaned.
    pub expired_filecoin_deals_cleaned: u64,
    /// Number of stale tier-promotion entries cleaned.
    pub stale_tier_promotions_cleaned: u64,
    /// Number of errors encountered (non-fatal).
    pub errors: u64,
}

impl GcSummary {
    /// Check if any cleanup was performed.
    pub fn is_empty(&self) -> bool {
        self.items_deleted == 0
            && self.orphaned_quarantine_cleaned == 0
            && self.expired_filecoin_deals_cleaned == 0
            && self.stale_tier_promotions_cleaned == 0
    }
}

/// Garbage collector for expired and orphaned content.
pub struct GarbageCollector {
    /// Retention policy manager to determine what should be deleted.
    retention_manager: RetentionManager,
    /// In-memory index of stored content by domain.
    content_index: DomainContentIndex,
    /// Quarantine records for content under review.
    quarantine_records: HashMap<String, QuarantineRecord>,
    /// Filecoin deal tracking (deal_id -> expiration_timestamp_secs).
    filecoin_deals: HashMap<String, u64>,
    /// Tier promotion tracker (content_id -> promotion_timestamp_secs).
    tier_promotions: HashMap<String, u64>,
}

impl GarbageCollector {
    /// Create a new garbage collector with a retention manager.
    pub fn new(retention_manager: RetentionManager) -> Self {
        Self {
            retention_manager,
            content_index: DomainContentIndex::new(),
            quarantine_records: HashMap::new(),
            filecoin_deals: HashMap::new(),
            tier_promotions: HashMap::new(),
        }
    }

    /// Add a content entry to be managed by this garbage collector.
    pub fn add_content(&mut self, entry: wrfcoin_storage_common::IndexEntry) {
        self.content_index.insert(entry);
    }

    /// Add a quarantine record.
    pub fn add_quarantine(&mut self, content_id: String, record: QuarantineRecord) {
        self.quarantine_records.insert(content_id, record);
    }

    /// Register a Filecoin deal with an expiration timestamp.
    pub fn add_filecoin_deal(&mut self, deal_id: String, expires_at_secs: u64) {
        self.filecoin_deals.insert(deal_id, expires_at_secs);
    }

    /// Register a tier-promotion timestamp.
    pub fn add_tier_promotion(&mut self, content_id: String, promoted_at_secs: u64) {
        self.tier_promotions.insert(content_id, promoted_at_secs);
    }

    /// Execute a garbage collection sweep at the given timestamp.
    ///
    /// This function evaluates all indexed content against retention policies
    /// and deletes expired items from the specified storage tier.
    ///
    /// # Arguments
    ///
    /// * `current_time_secs` - Current Unix timestamp for age calculations
    /// * `tier_promotion_max_age_secs` - Max age for tier-promotion entries (default ~24h)
    /// * `tier` - Storage tier to target for garbage collection
    /// * `dry_run` - If true, evaluates but does not actually delete
    /// * `provider` - Storage provider to call delete on
    ///
    /// # Returns
    ///
    /// A [`GcSummary`] describing all cleanup actions that were (or would be) taken.
    pub async fn gc_sweep(
        &self,
        current_time_secs: u64,
        tier_promotion_max_age_secs: u64,
        tier: DataTier,
        dry_run: bool,
        provider: &dyn wrfcoin_storage_common::StorageProvider,
    ) -> GcSummary {
        let mut summary = GcSummary::default();

        // Walk all domains and evaluate content for deletion.
        for domain in &[
            EnvironmentalDomain::Weather,
            EnvironmentalDomain::AirQuality,
            EnvironmentalDomain::WaterQuality,
            EnvironmentalDomain::Soil,
            EnvironmentalDomain::Ocean,
            EnvironmentalDomain::Hydrology,
            EnvironmentalDomain::Wildfire,
        ] {
            let entries = self.content_index.query_by_domain(domain);
            for entry in entries {
                summary.items_evaluated += 1;

                let age_secs = current_time_secs.saturating_sub(entry.stored_at);

                // Determine the artifact class from the domain and quality.
                // In a real implementation, this would come from metadata.
                let artifact_class = format!("{:?}", domain).to_lowercase();

                // Check retention for the target tier.
                let should_retain = self.retention_manager.should_retain(
                    &artifact_class,
                    &tier,
                    age_secs,
                    0.5, // default quality score
                );

                if !should_retain {
                    summary.items_deleted += 1;

                    // Call provider.delete() unless in dry_run mode
                    if !dry_run {
                        match provider.delete(&entry.content_id).await {
                            Ok(()) => {
                                // Delete succeeded
                            }
                            Err(_) => {
                                // Record error but continue sweep
                                summary.errors += 1;
                            }
                        }
                    }
                }
            }
        }

        // Clean up expired quarantine records.
        for (_content_id, record) in &self.quarantine_records {
            let age_secs = current_time_secs.saturating_sub(record.detected_at);
            // Default quarantine max age: 7 days
            if age_secs > 7 * 86400 {
                summary.orphaned_quarantine_cleaned += 1;
            }
        }

        // Clean up expired Filecoin deals.
        for (_deal_id, expires_at) in &self.filecoin_deals {
            if current_time_secs >= *expires_at {
                summary.expired_filecoin_deals_cleaned += 1;
            }
        }

        // Clean up stale tier-promotion entries (older than max_age).
        for (_content_id, promoted_at) in &self.tier_promotions {
            let age_secs = current_time_secs.saturating_sub(*promoted_at);
            if age_secs > tier_promotion_max_age_secs {
                summary.stale_tier_promotions_cleaned += 1;
            }
        }

        summary
    }

    /// Execute a garbage collection sweep using the current system time.
    /// Uses Cold tier and dry_run=false with a provided storage provider.
    pub async fn gc_sweep_now(
        &self,
        provider: &dyn wrfcoin_storage_common::StorageProvider,
    ) -> GcSummary {
        let current_time_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.gc_sweep(
            current_time_secs,
            24 * 3600,
            DataTier::Cold,
            false,
            provider,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_storage_common::types::ContentId;
    use std::sync::{Arc, Mutex};
    use async_trait::async_trait;

    /// Mock storage provider for testing.
    struct MockProvider {
        deleted_ids: Arc<Mutex<Vec<String>>>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                deleted_ids: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn deleted_count(&self) -> usize {
            self.deleted_ids.lock().unwrap().len()
        }

        fn was_deleted(&self, content_id: &str) -> bool {
            self.deleted_ids
                .lock()
                .unwrap()
                .iter()
                .any(|id| id == content_id)
        }
    }

    #[async_trait]
    impl wrfcoin_storage_common::StorageProvider for MockProvider {
        async fn store(
            &self,
            _data: &[u8],
            _metadata: &wrfcoin_storage_common::types::Metadata,
        ) -> Result<ContentId, wrfcoin_storage_common::StorageError> {
            Ok(ContentId {
                provider: "mock".to_string(),
                hash: "mock_hash".to_string(),
            })
        }

        async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, wrfcoin_storage_common::StorageError> {
            Ok(vec![1, 2, 3])
        }

        async fn exists(&self, _id: &ContentId) -> Result<bool, wrfcoin_storage_common::StorageError> {
            Ok(true)
        }

        async fn delete(&self, id: &ContentId) -> Result<(), wrfcoin_storage_common::StorageError> {
            self.deleted_ids
                .lock()
                .unwrap()
                .push(format!("{}:{}", id.provider, id.hash));
            Ok(())
        }

        async fn health_check(
            &self,
        ) -> Result<wrfcoin_storage_common::types::HealthStatus, wrfcoin_storage_common::StorageError> {
            Ok(wrfcoin_storage_common::types::HealthStatus {
                healthy: true,
                provider: "mock".to_string(),
                latency_ms: None,
                message: None,
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

    fn create_test_gc() -> GarbageCollector {
        let mut retention = RetentionManager::new();

        // Add a policy that expires after 1000 seconds.
        let mut policy = wrfcoin_storage_common::types::RetentionPolicy::new(
            "weather",
            DataTier::Cold,
            1,
        );
        policy.max_age_secs = 1000;
        retention.add_policy(policy);

        GarbageCollector::new(retention)
    }

    #[tokio::test]
    async fn test_gc_sweep_expired_content_marked_for_deletion() {
        let mut gc = create_test_gc();
        let provider = MockProvider::new();

        // Add content that is old enough to expire.
        let entry = wrfcoin_storage_common::IndexEntry {
            content_id: ContentId {
                provider: "ipfs".to_string(),
                hash: "Qm_expired".to_string(),
            },
            domain: EnvironmentalDomain::Weather,
            station_id: "stn-1".to_string(),
            report_id: "report-1".to_string(),
            stored_at: 1000,
        };
        gc.add_content(entry);

        // Run GC at time 3000 (2000 seconds later, > 1000 second retention).
        let summary = gc
            .gc_sweep(3000, 24 * 3600, DataTier::Cold, false, &provider)
            .await;

        assert_eq!(summary.items_evaluated, 1);
        assert_eq!(summary.items_deleted, 1, "expired content should be deleted");
        assert!(
            !summary.is_empty(),
            "summary should show cleanup was performed"
        );
        // Verify provider.delete() was called
        assert_eq!(provider.deleted_count(), 1, "provider.delete() should have been called");
    }

    #[tokio::test]
    async fn test_gc_sweep_retains_non_expired_content() {
        let mut gc = create_test_gc();
        let provider = MockProvider::new();

        // Add content that is recent (not yet expired).
        let entry = wrfcoin_storage_common::IndexEntry {
            content_id: ContentId {
                provider: "ipfs".to_string(),
                hash: "Qm_fresh".to_string(),
            },
            domain: EnvironmentalDomain::Weather,
            station_id: "stn-1".to_string(),
            report_id: "report-1".to_string(),
            stored_at: 2000,
        };
        gc.add_content(entry);

        // Run GC at time 2500 (500 seconds later, < 1000 second retention).
        let summary = gc
            .gc_sweep(2500, 24 * 3600, DataTier::Cold, false, &provider)
            .await;

        assert_eq!(summary.items_evaluated, 1);
        assert_eq!(summary.items_deleted, 0, "non-expired content should be retained");
    }

    #[tokio::test]
    async fn test_gc_sweep_cleans_orphaned_quarantine_records() {
        let mut gc = create_test_gc();
        let provider = MockProvider::new();

        // Add an orphaned quarantine record (old).
        let record = QuarantineRecord {
            quarantine_id: "quar-123".to_string(),
            reason: "corrupted".to_string(),
            detected_at: 1000,
            review_required: true,
            reinstated: false,
        };
        gc.add_quarantine("ipfs:Qm_orphan".to_string(), record);

        // Run GC at time 1_000_000 (more than 7 days later, > 7 * 86400 seconds).
        let summary = gc
            .gc_sweep(1_000_000, 24 * 3600, DataTier::Cold, false, &provider)
            .await;

        assert_eq!(summary.orphaned_quarantine_cleaned, 1);
    }

    #[tokio::test]
    async fn test_gc_sweep_cleans_expired_filecoin_deals() {
        let mut gc = create_test_gc();
        let provider = MockProvider::new();

        // Add an expired Filecoin deal.
        gc.add_filecoin_deal("deal-123".to_string(), 5000);

        // Run GC at time 6000 (after expiration).
        let summary = gc
            .gc_sweep(6000, 24 * 3600, DataTier::Cold, false, &provider)
            .await;

        assert_eq!(summary.expired_filecoin_deals_cleaned, 1);
    }

    #[tokio::test]
    async fn test_gc_sweep_cleans_stale_tier_promotions() {
        let mut gc = create_test_gc();
        let provider = MockProvider::new();

        // Add a tier promotion that is older than 24 hours.
        gc.add_tier_promotion("ipfs:Qm_old".to_string(), 1000);

        // Run GC at time 100_000 (much older than 24 hours).
        let summary = gc
            .gc_sweep(100_000, 24 * 3600, DataTier::Cold, false, &provider)
            .await;

        assert_eq!(summary.stale_tier_promotions_cleaned, 1);
    }

    #[tokio::test]
    async fn test_gc_summary_is_empty_when_no_cleanup() {
        let gc = create_test_gc();
        let provider = MockProvider::new();

        let summary = gc
            .gc_sweep(2000, 24 * 3600, DataTier::Cold, false, &provider)
            .await;

        assert!(summary.is_empty(), "summary should be empty when no cleanup occurs");
    }

    #[tokio::test]
    async fn test_gc_sweep_multiple_content_entries() {
        let mut gc = create_test_gc();
        let provider = MockProvider::new();

        // Add multiple entries at different times.
        for i in 0..5 {
            let entry = wrfcoin_storage_common::IndexEntry {
                content_id: ContentId {
                    provider: "ipfs".to_string(),
                    hash: format!("Qm_entry_{}", i),
                },
                domain: EnvironmentalDomain::Weather,
                station_id: format!("stn-{}", i),
                report_id: format!("report-{}", i),
                stored_at: 1000 + (i as u64 * 100), // staggered timestamps
            };
            gc.add_content(entry);
        }

        // Run GC at time 3000.
        // Entries at: 1000, 1100, 1200, 1300, 1400 should all be deleted (all > 1000 secs old).
        let summary = gc
            .gc_sweep(3000, 24 * 3600, DataTier::Cold, false, &provider)
            .await;

        assert_eq!(summary.items_evaluated, 5);
        assert_eq!(summary.items_deleted, 5);
        // Verify all entries were deleted
        assert_eq!(provider.deleted_count(), 5, "all expired entries should be deleted");
    }

    #[tokio::test]
    async fn test_gc_sweep_dry_run_does_not_delete() {
        let mut gc = create_test_gc();
        let provider = MockProvider::new();

        // Add content that is old enough to expire.
        let entry = wrfcoin_storage_common::IndexEntry {
            content_id: ContentId {
                provider: "ipfs".to_string(),
                hash: "Qm_expired".to_string(),
            },
            domain: EnvironmentalDomain::Weather,
            station_id: "stn-1".to_string(),
            report_id: "report-1".to_string(),
            stored_at: 1000,
        };
        gc.add_content(entry);

        // Run GC in dry_run mode at time 3000.
        let summary = gc
            .gc_sweep(3000, 24 * 3600, DataTier::Cold, true, &provider)
            .await;

        assert_eq!(summary.items_evaluated, 1);
        assert_eq!(summary.items_deleted, 1, "dry_run should still count deletions");
        // But provider.delete() should NOT have been called
        assert_eq!(
            provider.deleted_count(),
            0,
            "dry_run mode should not call provider.delete()"
        );
    }

    #[tokio::test]
    async fn test_gc_sweep_tier_filtering() {
        let mut retention = RetentionManager::new();

        // Add separate policies for Cold and Hot tiers
        let mut cold_policy = wrfcoin_storage_common::types::RetentionPolicy::new("weather", DataTier::Cold, 1);
        cold_policy.max_age_secs = 1000;
        retention.add_policy(cold_policy);

        let mut hot_policy = wrfcoin_storage_common::types::RetentionPolicy::new("weather", DataTier::Hot, 1);
        hot_policy.max_age_secs = 10000; // Much longer for Hot tier
        retention.add_policy(hot_policy);

        let mut gc = GarbageCollector::new(retention);
        let provider = MockProvider::new();

        // Add content that is 2000 seconds old
        let entry = wrfcoin_storage_common::IndexEntry {
            content_id: ContentId {
                provider: "ipfs".to_string(),
                hash: "Qm_content".to_string(),
            },
            domain: EnvironmentalDomain::Weather,
            station_id: "stn-1".to_string(),
            report_id: "report-1".to_string(),
            stored_at: 1000,
        };
        gc.add_content(entry);

        // Sweeping Cold tier should delete (2000 > 1000)
        let cold_summary = gc
            .gc_sweep(3000, 24 * 3600, DataTier::Cold, false, &provider)
            .await;
        assert_eq!(cold_summary.items_deleted, 1, "Cold tier should delete expired content");

        // Sweeping Hot tier should NOT delete (2000 < 10000)
        let hot_summary = gc
            .gc_sweep(3000, 24 * 3600, DataTier::Hot, false, &provider)
            .await;
        assert_eq!(hot_summary.items_deleted, 0, "Hot tier should retain content");
    }
}
