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

    /// Create a retention manager pre-loaded with default policies for all
    /// environmental observation domains.
    pub fn with_environmental_defaults() -> Self {
        let mut mgr = Self::new();

        // Weather — high volume, moderate retention
        let mut weather = RetentionPolicy::new("weather.observation", DataTier::Cold, 2);
        weather.max_age_secs = 180 * 86400; // 180 days
        mgr.add_policy(weather);

        // Air quality — regulatory value, longer retention
        let mut aq = RetentionPolicy::new("air_quality.observation", DataTier::Cold, 2);
        aq.max_age_secs = 365 * 86400; // 1 year
        mgr.add_policy(aq);

        // Water quality — environmental monitoring
        let mut wq = RetentionPolicy::new("water_quality.observation", DataTier::Cold, 2);
        wq.max_age_secs = 365 * 86400;
        mgr.add_policy(wq);

        // Wildfire — critical safety data
        let mut wildfire = RetentionPolicy::new("wildfire.observation", DataTier::Cold, 3);
        wildfire.max_age_secs = 365 * 86400;
        mgr.add_policy(wildfire);

        // Soil — long-term agricultural/climate value
        let mut soil = RetentionPolicy::new("soil.observation", DataTier::Cold, 2);
        soil.max_age_secs = 0; // never expires
        mgr.add_policy(soil);
        let mut soil_archive = RetentionPolicy::new("soil.observation", DataTier::Archive, 2);
        soil_archive.max_age_secs = 0;
        mgr.add_policy(soil_archive);

        // Ocean — climate science, long-term
        let mut ocean = RetentionPolicy::new("ocean.observation", DataTier::Cold, 2);
        ocean.max_age_secs = 0; // never expires
        mgr.add_policy(ocean);
        let mut ocean_archive = RetentionPolicy::new("ocean.observation", DataTier::Archive, 2);
        ocean_archive.max_age_secs = 0;
        mgr.add_policy(ocean_archive);

        // Hydrology — flood risk, high redundancy
        let mut hydro = RetentionPolicy::new("hydrology.observation", DataTier::Cold, 3);
        hydro.max_age_secs = 0; // never expires
        mgr.add_policy(hydro);
        let mut hydro_archive = RetentionPolicy::new("hydrology.observation", DataTier::Archive, 3);
        hydro_archive.max_age_secs = 0;
        mgr.add_policy(hydro_archive);

        mgr
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

    #[test]
    fn test_environmental_defaults_include_soil() {
        let mgr = RetentionManager::with_environmental_defaults();
        let policies = mgr.policies_for("soil.observation");
        assert!(!policies.is_empty());
        assert!(policies[0].min_copies >= 2);
    }

    #[test]
    fn test_environmental_defaults_include_ocean() {
        let mgr = RetentionManager::with_environmental_defaults();
        let policies = mgr.policies_for("ocean.observation");
        assert!(!policies.is_empty());
    }

    #[test]
    fn test_environmental_defaults_include_hydrology() {
        let mgr = RetentionManager::with_environmental_defaults();
        let policies = mgr.policies_for("hydrology.observation");
        assert!(!policies.is_empty());
    }

    #[test]
    fn test_environmental_defaults_soil_long_retention() {
        let mgr = RetentionManager::with_environmental_defaults();
        let policies = mgr.policies_for("soil.observation");
        // Soil data is valuable long-term — max_age_secs should be >= 1 year or 0 (never expires)
        assert!(policies.iter().any(|p| p.max_age_secs == 0 || p.max_age_secs >= 365 * 86400));
    }

    #[test]
    fn test_environmental_defaults_hydrology_min_copies() {
        let mgr = RetentionManager::with_environmental_defaults();
        // Hydrology (flood risk) data needs redundancy
        assert!(mgr.min_copies_for("hydrology.observation") >= 3);
    }
}
