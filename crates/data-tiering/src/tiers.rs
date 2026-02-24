use serde::{Deserialize, Serialize};
use wrfcoin_storage_common::DataTier;

/// Configuration for a single storage tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierConfig {
    /// Which data tier this configuration applies to.
    pub tier: DataTier,
    /// Maximum total size in bytes for this tier.
    pub max_size_bytes: u64,
    /// Maximum age in seconds before content should be migrated to a colder tier.
    pub max_age_secs: u64,
    /// Cost per GiB for storage in this tier.
    pub cost_per_gib: f64,
    /// Expected access latency in milliseconds.
    pub access_time_ms: u64,
}

/// Aggregate tiering configuration across all tiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieringConfig {
    /// Configuration for the hot tier.
    pub hot: TierConfig,
    /// Configuration for the warm tier.
    pub warm: TierConfig,
    /// Configuration for the cold tier.
    pub cold: TierConfig,
    /// Configuration for the archive tier.
    pub archive: TierConfig,
    /// Whether to automatically migrate data between tiers based on age and access patterns.
    pub auto_migrate: bool,
}

impl Default for TieringConfig {
    fn default() -> Self {
        Self {
            hot: TierConfig {
                tier: DataTier::Hot,
                max_size_bytes: 10 * 1024 * 1024 * 1024, // 10 GiB
                max_age_secs: 86_400,                      // 1 day
                cost_per_gib: 0.10,
                access_time_ms: 5,
            },
            warm: TierConfig {
                tier: DataTier::Warm,
                max_size_bytes: 100 * 1024 * 1024 * 1024, // 100 GiB
                max_age_secs: 604_800,                      // 7 days
                cost_per_gib: 0.03,
                access_time_ms: 50,
            },
            cold: TierConfig {
                tier: DataTier::Cold,
                max_size_bytes: 1024 * 1024 * 1024 * 1024, // 1 TiB
                max_age_secs: 2_592_000,                     // 30 days
                cost_per_gib: 0.005,
                access_time_ms: 500,
            },
            archive: TierConfig {
                tier: DataTier::Archive,
                max_size_bytes: u64::MAX,
                max_age_secs: u64::MAX,
                cost_per_gib: 0.001,
                access_time_ms: 5000,
            },
            auto_migrate: true,
        }
    }
}

impl TieringConfig {
    /// Look up the configuration for a specific tier.
    pub fn config_for(&self, tier: &DataTier) -> &TierConfig {
        match tier {
            DataTier::Hot => &self.hot,
            DataTier::Warm => &self.warm,
            DataTier::Cold => &self.cold,
            DataTier::Archive => &self.archive,
        }
    }
}
