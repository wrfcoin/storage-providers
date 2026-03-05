//! Retention policy management for archived weather data.

use wrfcoin_storage_common::types::{DataTier, RetentionPolicy};

/// Manages retention policies by artifact class and tier.
pub struct RetentionManager {
    policies: Vec<RetentionPolicy>,
}

impl RetentionManager {
    /// Create an empty retention manager.
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    /// Add a retention policy.
    pub fn add_policy(&mut self, policy: RetentionPolicy) {
        self.policies.push(policy);
    }

    /// Get policies matching the given artifact class.
    pub fn policies_for(&self, artifact_class: &str) -> Vec<&RetentionPolicy> {
        self.policies
            .iter()
            .filter(|p| p.artifact_class == artifact_class)
            .collect()
    }

    /// Check if data should be retained based on all matching policies.
    ///
    /// Returns `true` if any matching policy says the data should be retained.
    /// If no policies match, returns `true` (retain by default).
    pub fn should_retain(
        &self,
        artifact_class: &str,
        tier: &DataTier,
        age_secs: u64,
        quality_score: f64,
    ) -> bool {
        let matching: Vec<_> = self
            .policies
            .iter()
            .filter(|p| p.artifact_class == artifact_class && p.tier == *tier)
            .collect();

        if matching.is_empty() {
            return true; // No policy = retain by default
        }

        matching
            .iter()
            .any(|p| p.should_retain(age_secs, quality_score))
    }

    /// Get minimum copies required for the given artifact class.
    ///
    /// Returns the maximum `min_copies` across all matching policies,
    /// or 1 if no policies match.
    pub fn min_copies_for(&self, artifact_class: &str) -> usize {
        self.policies
            .iter()
            .filter(|p| p.artifact_class == artifact_class)
            .map(|p| p.min_copies)
            .max()
            .unwrap_or(1)
    }
}

impl Default for RetentionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_manager_retains_by_default() {
        let mgr = RetentionManager::new();
        assert!(mgr.should_retain("forecast", &DataTier::Archive, 99999, 0.5));
    }

    #[test]
    fn test_empty_manager_min_copies_default() {
        let mgr = RetentionManager::new();
        assert_eq!(mgr.min_copies_for("forecast"), 1);
    }

    #[test]
    fn test_add_and_query_policy() {
        let mut mgr = RetentionManager::new();
        let mut policy = RetentionPolicy::new("forecast", DataTier::Archive, 3);
        policy.max_age_secs = 86400;
        mgr.add_policy(policy);

        let found = mgr.policies_for("forecast");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].min_copies, 3);
    }

    #[test]
    fn test_should_retain_within_age() {
        let mut mgr = RetentionManager::new();
        let mut policy = RetentionPolicy::new("forecast", DataTier::Archive, 2);
        policy.max_age_secs = 3600;
        mgr.add_policy(policy);

        assert!(mgr.should_retain("forecast", &DataTier::Archive, 1800, 0.5));
    }

    #[test]
    fn test_should_not_retain_expired() {
        let mut mgr = RetentionManager::new();
        let mut policy = RetentionPolicy::new("forecast", DataTier::Archive, 2);
        policy.max_age_secs = 3600;
        mgr.add_policy(policy);

        assert!(!mgr.should_retain("forecast", &DataTier::Archive, 7200, 0.5));
    }

    #[test]
    fn test_should_not_retain_low_quality() {
        let mut mgr = RetentionManager::new();
        let mut policy = RetentionPolicy::new("observation", DataTier::Cold, 1);
        policy.min_quality_score = 0.5;
        mgr.add_policy(policy);

        assert!(!mgr.should_retain("observation", &DataTier::Cold, 100, 0.3));
    }

    #[test]
    fn test_min_copies_takes_max() {
        let mut mgr = RetentionManager::new();
        mgr.add_policy(RetentionPolicy::new("forecast", DataTier::Archive, 2));
        mgr.add_policy(RetentionPolicy::new("forecast", DataTier::Cold, 5));

        assert_eq!(mgr.min_copies_for("forecast"), 5);
    }

    #[test]
    fn test_unmatched_tier_retains() {
        let mut mgr = RetentionManager::new();
        let mut policy = RetentionPolicy::new("forecast", DataTier::Archive, 2);
        policy.max_age_secs = 3600;
        mgr.add_policy(policy);

        // Policy is for Archive tier, querying Hot tier should default to retain
        assert!(mgr.should_retain("forecast", &DataTier::Hot, 99999, 0.5));
    }
}
