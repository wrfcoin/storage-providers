use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use wrfcoin_storage_common::DataTier;

/// Trust class for data contributors.
///
/// Determines retention duration and access rights across storage tiers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TrustClass {
    /// Highest trust — longest retention, unrestricted access.
    Verified,
    /// Default trust level.
    Standard,
    /// Reduced trust — shorter retention windows.
    Probation,
    /// Minimal trust — archive-only retention and access.
    Untrusted,
}

/// Quality-aware retention policy for a specific trust class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityRetentionPolicy {
    /// The trust class this policy applies to.
    pub trust_class: TrustClass,
    /// Number of hours content is kept in the Hot tier.
    pub hot_retention_hours: u64,
    /// Number of hours content is kept in the Warm tier.
    pub warm_retention_hours: u64,
    /// Number of days content is kept in the Cold tier.
    pub cold_retention_days: u64,
    /// Number of days content is kept in the Archive tier.
    pub archive_retention_days: u64,
    /// Maximum number of replicas to maintain.
    pub max_replicas: u8,
}

/// Quality-aware access control policy for a specific trust class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessControlPolicy {
    /// The trust class this policy applies to.
    pub trust_class: TrustClass,
    /// Whether the trust class may read from the Hot tier.
    pub can_read_hot: bool,
    /// Whether the trust class may read from the Warm tier.
    pub can_read_warm: bool,
    /// Whether the trust class may read from the Cold tier.
    pub can_read_cold: bool,
    /// Whether the trust class may read from the Archive tier.
    pub can_read_archive: bool,
    /// Maximum number of read requests allowed per hour.
    pub max_requests_per_hour: u32,
}

/// Manages retention and access-control policies keyed by trust class.
pub struct QualityRetentionManager {
    retention_policies: HashMap<TrustClass, QualityRetentionPolicy>,
    access_policies: HashMap<TrustClass, AccessControlPolicy>,
}

impl QualityRetentionManager {
    /// Create a new manager pre-populated with sensible default policies for
    /// all four trust classes.
    pub fn new() -> Self {
        let mut mgr = Self {
            retention_policies: HashMap::new(),
            access_policies: HashMap::new(),
        };
        mgr.load_defaults();
        mgr
    }

    fn load_defaults(&mut self) {
        // --- Retention defaults ---
        let retention_defaults = [
            QualityRetentionPolicy {
                trust_class: TrustClass::Verified,
                hot_retention_hours: 168,   // 7 days
                warm_retention_hours: 2160, // 90 days
                cold_retention_days: 730,   // 2 years
                archive_retention_days: u64::MAX,
                max_replicas: 5,
            },
            QualityRetentionPolicy {
                trust_class: TrustClass::Standard,
                hot_retention_hours: 48,   // 2 days
                warm_retention_hours: 720, // 30 days
                cold_retention_days: 365,  // 1 year
                archive_retention_days: 3650,
                max_replicas: 3,
            },
            QualityRetentionPolicy {
                trust_class: TrustClass::Probation,
                hot_retention_hours: 6,   // 6 hours
                warm_retention_hours: 72, // 3 days
                cold_retention_days: 30,  // 1 month
                archive_retention_days: 365,
                max_replicas: 2,
            },
            QualityRetentionPolicy {
                trust_class: TrustClass::Untrusted,
                hot_retention_hours: 0,  // not eligible for Hot
                warm_retention_hours: 0, // not eligible for Warm
                cold_retention_days: 0,  // not eligible for Cold
                archive_retention_days: 90,
                max_replicas: 1,
            },
        ];

        for p in retention_defaults {
            self.retention_policies.insert(p.trust_class.clone(), p);
        }

        // --- Access-control defaults ---
        let access_defaults = [
            AccessControlPolicy {
                trust_class: TrustClass::Verified,
                can_read_hot: true,
                can_read_warm: true,
                can_read_cold: true,
                can_read_archive: true,
                max_requests_per_hour: 10_000,
            },
            AccessControlPolicy {
                trust_class: TrustClass::Standard,
                can_read_hot: true,
                can_read_warm: true,
                can_read_cold: true,
                can_read_archive: true,
                max_requests_per_hour: 1_000,
            },
            AccessControlPolicy {
                trust_class: TrustClass::Probation,
                can_read_hot: false,
                can_read_warm: true,
                can_read_cold: true,
                can_read_archive: true,
                max_requests_per_hour: 100,
            },
            AccessControlPolicy {
                trust_class: TrustClass::Untrusted,
                can_read_hot: false,
                can_read_warm: false,
                can_read_cold: false,
                can_read_archive: true,
                max_requests_per_hour: 10,
            },
        ];

        for p in access_defaults {
            self.access_policies.insert(p.trust_class.clone(), p);
        }
    }

    /// Replace or insert a retention policy for the trust class it targets.
    pub fn set_retention_policy(&mut self, policy: QualityRetentionPolicy) {
        self.retention_policies.insert(policy.trust_class.clone(), policy);
    }

    /// Replace or insert an access-control policy for the trust class it targets.
    pub fn set_access_policy(&mut self, policy: AccessControlPolicy) {
        self.access_policies.insert(policy.trust_class.clone(), policy);
    }

    /// Suggest the storage tier for content from `trust_class` that is
    /// `age_hours` old.
    ///
    /// Walks from Hot → Warm → Cold → Archive, returning the hottest tier whose
    /// retention window still covers `age_hours`.  Falls back to Archive when
    /// no warmer tier applies.
    pub fn suggested_tier(&self, trust_class: &TrustClass, age_hours: u64) -> DataTier {
        let Some(policy) = self.retention_policies.get(trust_class) else {
            return DataTier::Archive;
        };

        if policy.hot_retention_hours > 0 && age_hours < policy.hot_retention_hours {
            return DataTier::Hot;
        }
        if policy.warm_retention_hours > 0 && age_hours < policy.warm_retention_hours {
            return DataTier::Warm;
        }
        let age_days = age_hours / 24;
        if policy.cold_retention_days > 0 && age_days < policy.cold_retention_days {
            return DataTier::Cold;
        }

        DataTier::Archive
    }

    /// Return whether `trust_class` is permitted to read from `tier`.
    pub fn can_access(&self, trust_class: &TrustClass, tier: &DataTier) -> bool {
        let Some(policy) = self.access_policies.get(trust_class) else {
            return false;
        };
        match tier {
            DataTier::Hot => policy.can_read_hot,
            DataTier::Warm => policy.can_read_warm,
            DataTier::Cold => policy.can_read_cold,
            DataTier::Archive => policy.can_read_archive,
        }
    }

    /// Look up the retention policy for `trust_class`.
    pub fn get_retention_policy(&self, trust_class: &TrustClass) -> Option<&QualityRetentionPolicy> {
        self.retention_policies.get(trust_class)
    }

    /// Look up the access-control policy for `trust_class`.
    pub fn get_access_policy(&self, trust_class: &TrustClass) -> Option<&AccessControlPolicy> {
        self.access_policies.get(trust_class)
    }
}

impl Default for QualityRetentionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Default policy tests ----

    #[test]
    fn test_default_policies_present_for_all_trust_classes() {
        let mgr = QualityRetentionManager::new();
        for tc in [
            TrustClass::Verified,
            TrustClass::Standard,
            TrustClass::Probation,
            TrustClass::Untrusted,
        ] {
            assert!(mgr.get_retention_policy(&tc).is_some(), "missing retention policy for {tc:?}");
            assert!(mgr.get_access_policy(&tc).is_some(), "missing access policy for {tc:?}");
        }
    }

    #[test]
    fn test_verified_has_longest_retention() {
        let mgr = QualityRetentionManager::new();
        let verified = mgr.get_retention_policy(&TrustClass::Verified).unwrap();
        let standard = mgr.get_retention_policy(&TrustClass::Standard).unwrap();
        let probation = mgr.get_retention_policy(&TrustClass::Probation).unwrap();
        assert!(verified.hot_retention_hours >= standard.hot_retention_hours);
        assert!(standard.hot_retention_hours >= probation.hot_retention_hours);
    }

    #[test]
    fn test_untrusted_cannot_access_hot_warm_cold() {
        let mgr = QualityRetentionManager::new();
        assert!(!mgr.can_access(&TrustClass::Untrusted, &DataTier::Hot));
        assert!(!mgr.can_access(&TrustClass::Untrusted, &DataTier::Warm));
        assert!(!mgr.can_access(&TrustClass::Untrusted, &DataTier::Cold));
        assert!(mgr.can_access(&TrustClass::Untrusted, &DataTier::Archive));
    }

    #[test]
    fn test_verified_can_access_all_tiers() {
        let mgr = QualityRetentionManager::new();
        assert!(mgr.can_access(&TrustClass::Verified, &DataTier::Hot));
        assert!(mgr.can_access(&TrustClass::Verified, &DataTier::Warm));
        assert!(mgr.can_access(&TrustClass::Verified, &DataTier::Cold));
        assert!(mgr.can_access(&TrustClass::Verified, &DataTier::Archive));
    }

    // ---- Custom policy tests ----

    #[test]
    fn test_set_retention_policy_overrides_default() {
        let mut mgr = QualityRetentionManager::new();
        mgr.set_retention_policy(QualityRetentionPolicy {
            trust_class: TrustClass::Standard,
            hot_retention_hours: 999,
            warm_retention_hours: 9999,
            cold_retention_days: 99,
            archive_retention_days: 9999,
            max_replicas: 7,
        });

        let p = mgr.get_retention_policy(&TrustClass::Standard).unwrap();
        assert_eq!(p.hot_retention_hours, 999);
        assert_eq!(p.max_replicas, 7);
    }

    #[test]
    fn test_set_access_policy_overrides_default() {
        let mut mgr = QualityRetentionManager::new();
        mgr.set_access_policy(AccessControlPolicy {
            trust_class: TrustClass::Probation,
            can_read_hot: true, // override default (false)
            can_read_warm: true,
            can_read_cold: true,
            can_read_archive: true,
            max_requests_per_hour: 50,
        });

        assert!(mgr.can_access(&TrustClass::Probation, &DataTier::Hot));
        let p = mgr.get_access_policy(&TrustClass::Probation).unwrap();
        assert_eq!(p.max_requests_per_hour, 50);
    }

    // ---- suggested_tier tests ----

    #[test]
    fn test_suggested_tier_verified_new_data_is_hot() {
        let mgr = QualityRetentionManager::new();
        // Age 1 hour — within Hot retention window.
        assert_eq!(mgr.suggested_tier(&TrustClass::Verified, 1), DataTier::Hot);
    }

    #[test]
    fn test_suggested_tier_verified_older_data_is_warm() {
        let mgr = QualityRetentionManager::new();
        let hot_hours = mgr.get_retention_policy(&TrustClass::Verified).unwrap().hot_retention_hours;
        // Just past the Hot window → should go to Warm.
        assert_eq!(mgr.suggested_tier(&TrustClass::Verified, hot_hours + 1), DataTier::Warm);
    }

    #[test]
    fn test_suggested_tier_standard_new_data_is_hot() {
        let mgr = QualityRetentionManager::new();
        assert_eq!(mgr.suggested_tier(&TrustClass::Standard, 0), DataTier::Hot);
    }

    #[test]
    fn test_suggested_tier_probation_new_data_is_hot() {
        let mgr = QualityRetentionManager::new();
        // Probation has a short Hot window; 0 hours should still be Hot.
        assert_eq!(mgr.suggested_tier(&TrustClass::Probation, 0), DataTier::Hot);
    }

    #[test]
    fn test_suggested_tier_probation_past_hot_warm_goes_cold() {
        let mgr = QualityRetentionManager::new();
        let p = mgr.get_retention_policy(&TrustClass::Probation).unwrap();
        // Past both Hot and Warm windows → Cold.
        let age = p.warm_retention_hours + 1;
        assert_eq!(mgr.suggested_tier(&TrustClass::Probation, age), DataTier::Cold);
    }

    #[test]
    fn test_suggested_tier_untrusted_always_archive() {
        let mgr = QualityRetentionManager::new();
        // Untrusted has 0 for Hot, Warm, Cold retention → always Archive.
        assert_eq!(mgr.suggested_tier(&TrustClass::Untrusted, 0), DataTier::Archive);
        assert_eq!(mgr.suggested_tier(&TrustClass::Untrusted, 10_000), DataTier::Archive);
    }

    #[test]
    fn test_suggested_tier_very_old_data_is_archive() {
        let mgr = QualityRetentionManager::new();
        // 100 years in hours — should always be Archive.
        let very_old = 100 * 365 * 24;
        assert_eq!(mgr.suggested_tier(&TrustClass::Verified, very_old), DataTier::Archive);
        assert_eq!(mgr.suggested_tier(&TrustClass::Standard, very_old), DataTier::Archive);
    }

    // ---- Access control per trust class ----

    #[test]
    fn test_probation_cannot_access_hot_by_default() {
        let mgr = QualityRetentionManager::new();
        assert!(!mgr.can_access(&TrustClass::Probation, &DataTier::Hot));
    }

    #[test]
    fn test_probation_can_access_warm_cold_archive_by_default() {
        let mgr = QualityRetentionManager::new();
        assert!(mgr.can_access(&TrustClass::Probation, &DataTier::Warm));
        assert!(mgr.can_access(&TrustClass::Probation, &DataTier::Cold));
        assert!(mgr.can_access(&TrustClass::Probation, &DataTier::Archive));
    }

    #[test]
    fn test_standard_can_access_all_tiers_by_default() {
        let mgr = QualityRetentionManager::new();
        assert!(mgr.can_access(&TrustClass::Standard, &DataTier::Hot));
        assert!(mgr.can_access(&TrustClass::Standard, &DataTier::Warm));
        assert!(mgr.can_access(&TrustClass::Standard, &DataTier::Cold));
        assert!(mgr.can_access(&TrustClass::Standard, &DataTier::Archive));
    }

    // ---- Edge cases ----

    #[test]
    fn test_can_access_unknown_trust_class_returns_false() {
        // After removing all policies, any lookup should return false gracefully.
        let mut mgr = QualityRetentionManager::new();
        mgr.access_policies.clear();
        assert!(!mgr.can_access(&TrustClass::Verified, &DataTier::Hot));
    }

    #[test]
    fn test_suggested_tier_unknown_trust_class_returns_archive() {
        let mut mgr = QualityRetentionManager::new();
        mgr.retention_policies.clear();
        assert_eq!(mgr.suggested_tier(&TrustClass::Verified, 0), DataTier::Archive);
    }

    #[test]
    fn test_serde_roundtrip_trust_class() {
        let tc = TrustClass::Probation;
        let json = serde_json::to_string(&tc).expect("serialize");
        let decoded: TrustClass = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, TrustClass::Probation);
    }

    #[test]
    fn test_serde_roundtrip_retention_policy() {
        let p = QualityRetentionPolicy {
            trust_class: TrustClass::Verified,
            hot_retention_hours: 100,
            warm_retention_hours: 200,
            cold_retention_days: 30,
            archive_retention_days: 365,
            max_replicas: 4,
        };
        let json = serde_json::to_string(&p).expect("serialize");
        let decoded: QualityRetentionPolicy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.trust_class, TrustClass::Verified);
        assert_eq!(decoded.hot_retention_hours, 100);
    }
}
