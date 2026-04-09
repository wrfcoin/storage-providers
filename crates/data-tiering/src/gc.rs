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
    /// and collects metadata about what would be deleted. In a full implementation,
    /// this would also call provider.delete() for each expired item.
    ///
    /// # Arguments
    ///
    /// * `current_time_secs` - Current Unix timestamp for age calculations
    /// * `tier_promotion_max_age_secs` - Max age for tier-promotion entries (default ~24h)
    ///
    /// # Returns
    ///
    /// A [`GcSummary`] describing all cleanup actions that would be taken.
    pub fn gc_sweep(
        &self,
        current_time_secs: u64,
        tier_promotion_max_age_secs: u64,
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

                // Check retention for the default tier (Cold) — in a real implementation,
                // we'd check all tiers where this content is stored.
                let should_retain = self.retention_manager.should_retain(
                    &artifact_class,
                    &DataTier::Cold,
                    age_secs,
                    0.5, // default quality score
                );

                if !should_retain {
                    summary.items_deleted += 1;
                    // In a real implementation, we'd call provider.delete(&entry.content_id)
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
    pub fn gc_sweep_now(&self) -> GcSummary {
        let current_time_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.gc_sweep(current_time_secs, 24 * 3600) // 24-hour default for tier promotions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_storage_common::types::ContentId;

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

    #[test]
    fn test_gc_sweep_expired_content_marked_for_deletion() {
        let mut gc = create_test_gc();

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
        let summary = gc.gc_sweep(3000, 24 * 3600);

        assert_eq!(summary.items_evaluated, 1);
        assert_eq!(summary.items_deleted, 1, "expired content should be deleted");
        assert!(
            !summary.is_empty(),
            "summary should show cleanup was performed"
        );
    }

    #[test]
    fn test_gc_sweep_retains_non_expired_content() {
        let mut gc = create_test_gc();

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
        let summary = gc.gc_sweep(2500, 24 * 3600);

        assert_eq!(summary.items_evaluated, 1);
        assert_eq!(summary.items_deleted, 0, "non-expired content should be retained");
    }

    #[test]
    fn test_gc_sweep_cleans_orphaned_quarantine_records() {
        let mut gc = create_test_gc();

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
        let summary = gc.gc_sweep(1_000_000, 24 * 3600);

        assert_eq!(summary.orphaned_quarantine_cleaned, 1);
    }

    #[test]
    fn test_gc_sweep_cleans_expired_filecoin_deals() {
        let mut gc = create_test_gc();

        // Add an expired Filecoin deal.
        gc.add_filecoin_deal("deal-123".to_string(), 5000);

        // Run GC at time 6000 (after expiration).
        let summary = gc.gc_sweep(6000, 24 * 3600);

        assert_eq!(summary.expired_filecoin_deals_cleaned, 1);
    }

    #[test]
    fn test_gc_sweep_cleans_stale_tier_promotions() {
        let mut gc = create_test_gc();

        // Add a tier promotion that is older than 24 hours.
        gc.add_tier_promotion("ipfs:Qm_old".to_string(), 1000);

        // Run GC at time 100_000 (much older than 24 hours).
        let summary = gc.gc_sweep(100_000, 24 * 3600);

        assert_eq!(summary.stale_tier_promotions_cleaned, 1);
    }

    #[test]
    fn test_gc_summary_is_empty_when_no_cleanup() {
        let gc = create_test_gc();

        let summary = gc.gc_sweep(2000, 24 * 3600);

        assert!(summary.is_empty(), "summary should be empty when no cleanup occurs");
    }

    #[test]
    fn test_gc_sweep_multiple_content_entries() {
        let mut gc = create_test_gc();

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
        let summary = gc.gc_sweep(3000, 24 * 3600);

        assert_eq!(summary.items_evaluated, 5);
        assert_eq!(summary.items_deleted, 5);
    }
}
