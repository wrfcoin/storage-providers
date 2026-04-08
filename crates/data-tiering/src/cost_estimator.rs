//! Storage cost estimation and cheapest-provider routing.
//!
//! Implements the [`StorageCostModel`] trait for IPFS, Filecoin, and Arweave, plus
//! a [`CostEstimator`] that compares providers and routes to the cheapest healthy one
//! within an optional budget ceiling.

use std::collections::HashMap;

use async_trait::async_trait;
use wrfcoin_storage_common::StorageError;

// ── Constants ──────────────────────────────────────────────────────────────────

/// attoFIL units per 1 FIL.
const ATTOFIT_PER_FIL: f64 = 1e18;
/// Filecoin epoch length in seconds.
const SECS_PER_EPOCH: u64 = 30;
/// Winston units per 1 AR.
const WINSTON_PER_AR: f64 = 1e12;

// ── CostEstimate ───────────────────────────────────────────────────────────────

/// A point-in-time cost estimate for storing `size_bytes` for `duration_secs`.
#[derive(Debug, Clone, PartialEq)]
pub struct CostEstimate {
    /// Provider name (e.g. `"ipfs"`, `"filecoin"`, `"arweave"`).
    pub provider: String,
    /// One-time upfront cost (e.g. Arweave permanent storage fee).
    pub one_time_cost: f64,
    /// Recurring cost per calendar day (e.g. Filecoin epoch fees, remote pin).
    pub recurring_cost_per_day: f64,
    /// Total estimated cost over the requested duration.
    pub total_cost: f64,
    /// Currency / unit: `"USD"`, `"FIL"`, `"AR"`, or `"free"`.
    pub currency: String,
}

// ── StorageCostModel trait ─────────────────────────────────────────────────────

/// Cost model for a single storage provider.
///
/// Implementations may query external APIs or use local fallback estimates.
#[async_trait]
pub trait StorageCostModel: Send + Sync {
    /// Canonical provider name — must match the health-status key used by callers.
    fn provider_name(&self) -> &str;

    /// Estimate the cost to store `size_bytes` for `duration_secs`.
    async fn estimate_cost(
        &self,
        size_bytes: u64,
        duration_secs: u64,
    ) -> Result<CostEstimate, StorageError>;
}

// ── IPFS cost model ────────────────────────────────────────────────────────────

/// Cost model for IPFS storage.
///
/// Local nodes pin for free. Remote pinning services (Pinata, Web3.storage)
/// charge per GiB per month.
#[derive(Debug, Clone)]
pub struct IpfsCostModel {
    /// `true` when pinning to a locally-run node (free).
    pub is_local: bool,
    /// Remote pinning cost in USD per GiB per month. Ignored when `is_local`.
    pub remote_usd_per_gib_per_month: f64,
}

impl Default for IpfsCostModel {
    fn default() -> Self {
        Self {
            is_local: true,
            remote_usd_per_gib_per_month: 0.15, // typical Pinata pricing
        }
    }
}

#[async_trait]
impl StorageCostModel for IpfsCostModel {
    fn provider_name(&self) -> &str {
        "ipfs"
    }

    async fn estimate_cost(
        &self,
        size_bytes: u64,
        duration_secs: u64,
    ) -> Result<CostEstimate, StorageError> {
        if self.is_local {
            return Ok(CostEstimate {
                provider: "ipfs".to_string(),
                one_time_cost: 0.0,
                recurring_cost_per_day: 0.0,
                total_cost: 0.0,
                currency: "free".to_string(),
            });
        }

        let size_gib = size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let duration_days = duration_secs as f64 / 86_400.0;
        let duration_months = duration_days / 30.0;
        let total_cost = size_gib * self.remote_usd_per_gib_per_month * duration_months;
        let recurring_per_day = if duration_days > 0.0 {
            total_cost / duration_days
        } else {
            0.0
        };

        Ok(CostEstimate {
            provider: "ipfs".to_string(),
            one_time_cost: 0.0,
            recurring_cost_per_day: recurring_per_day,
            total_cost,
            currency: "USD".to_string(),
        })
    }
}

// ── Filecoin cost model ────────────────────────────────────────────────────────

/// Cost model for Filecoin storage deals.
///
/// Deal cost = `size_gib × price_per_epoch_in_fil × epochs`.
/// `max_price_per_gib_epoch` is in attoFIL/GiB/epoch, matching
/// `FilecoinConfig::max_price_per_gib`.
#[derive(Debug, Clone)]
pub struct FilecoinCostModel {
    /// Max price in attoFIL per GiB per epoch (from `FilecoinConfig::max_price_per_gib`).
    pub max_price_per_gib_epoch: u64,
    /// FIL/USD exchange rate. Set to `0.0` to report costs in FIL.
    pub fil_usd_rate: f64,
}

impl Default for FilecoinCostModel {
    fn default() -> Self {
        Self {
            max_price_per_gib_epoch: 1_000_000_000, // 1 nanoFIL/GiB/epoch
            fil_usd_rate: 0.0,
        }
    }
}

#[async_trait]
impl StorageCostModel for FilecoinCostModel {
    fn provider_name(&self) -> &str {
        "filecoin"
    }

    async fn estimate_cost(
        &self,
        size_bytes: u64,
        duration_secs: u64,
    ) -> Result<CostEstimate, StorageError> {
        let size_gib = size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let epochs = (duration_secs / SECS_PER_EPOCH).max(1) as f64;
        let price_per_gib_epoch_fil = self.max_price_per_gib_epoch as f64 / ATTOFIT_PER_FIL;
        let total_fil = size_gib * price_per_gib_epoch_fil * epochs;

        let (total_cost, currency) = if self.fil_usd_rate > 0.0 {
            (total_fil * self.fil_usd_rate, "USD".to_string())
        } else {
            (total_fil, "FIL".to_string())
        };

        let duration_days = duration_secs as f64 / 86_400.0;
        let recurring_per_day = if duration_days > 0.0 {
            total_cost / duration_days
        } else {
            total_cost
        };

        Ok(CostEstimate {
            provider: "filecoin".to_string(),
            one_time_cost: 0.0,
            recurring_cost_per_day: recurring_per_day,
            total_cost,
            currency,
        })
    }
}

// ── Arweave cost model ─────────────────────────────────────────────────────────

/// Cost model for Arweave permanent storage.
///
/// Arweave charges a single one-time fee for permanent storage; duration has no
/// effect on cost.
///
/// Set `gateway_winston` to the result of `ArweaveGatewayClient::get_price(bytes)`
/// for real-time pricing; otherwise the fallback `ar_per_kib` rate is used.
#[derive(Debug, Clone)]
pub struct ArweaveCostModel {
    /// Fallback rate in AR per KiB (matches `ArweaveStorageProvider::estimate_cost`).
    pub ar_per_kib: f64,
    /// Pre-fetched gateway price in winston. Overrides `ar_per_kib` when `Some`.
    pub gateway_winston: Option<u64>,
    /// AR/USD exchange rate. Set to `0.0` to report costs in AR.
    pub ar_usd_rate: f64,
}

impl Default for ArweaveCostModel {
    fn default() -> Self {
        Self {
            ar_per_kib: 0.0001,
            gateway_winston: None,
            ar_usd_rate: 0.0,
        }
    }
}

#[async_trait]
impl StorageCostModel for ArweaveCostModel {
    fn provider_name(&self) -> &str {
        "arweave"
    }

    async fn estimate_cost(
        &self,
        size_bytes: u64,
        _duration_secs: u64,
    ) -> Result<CostEstimate, StorageError> {
        let one_time_ar = match self.gateway_winston {
            Some(w) => w as f64 / WINSTON_PER_AR,
            None => (size_bytes as f64 / 1024.0) * self.ar_per_kib,
        };

        let (one_time_cost, currency) = if self.ar_usd_rate > 0.0 {
            (one_time_ar * self.ar_usd_rate, "USD".to_string())
        } else {
            (one_time_ar, "AR".to_string())
        };

        Ok(CostEstimate {
            provider: "arweave".to_string(),
            one_time_cost,
            recurring_cost_per_day: 0.0,
            total_cost: one_time_cost,
            currency,
        })
    }
}

// ── CostEstimator ──────────────────────────────────────────────────────────────

/// Compares storage costs across multiple providers and routes to the cheapest.
pub struct CostEstimator {
    models: Vec<Box<dyn StorageCostModel>>,
}

impl CostEstimator {
    /// Create a new estimator with no registered models.
    pub fn new() -> Self {
        Self { models: Vec::new() }
    }

    /// Register a cost model.
    pub fn add_model(&mut self, model: Box<dyn StorageCostModel>) {
        self.models.push(model);
    }

    /// Estimate costs for all registered providers, sorted cheapest-first by
    /// `total_cost`.
    ///
    /// Providers whose estimation fails are silently omitted.
    pub async fn compare(
        &self,
        size_bytes: u64,
        duration_secs: u64,
    ) -> Vec<(String, CostEstimate)> {
        let mut results = Vec::new();
        for model in &self.models {
            if let Ok(estimate) = model.estimate_cost(size_bytes, duration_secs).await {
                results.push((model.provider_name().to_string(), estimate));
            }
        }
        results.sort_by(|a, b| {
            a.1.total_cost
                .partial_cmp(&b.1.total_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Return the cheapest provider that passes health and budget checks.
    ///
    /// # Parameters
    /// - `health`: map of provider name → healthy. Providers absent from the map
    ///   are treated as healthy.
    /// - `max_budget`: optional total-cost ceiling. Comparison is in the model's
    ///   native currency; budget filtering is most useful when all models share
    ///   a common currency (e.g. all USD).
    ///
    /// Returns `None` if no provider passes both checks.
    pub async fn cheapest_provider(
        &self,
        size_bytes: u64,
        duration_secs: u64,
        health: &HashMap<String, bool>,
        max_budget: Option<f64>,
    ) -> Option<CostEstimate> {
        let ranked = self.compare(size_bytes, duration_secs).await;
        for (name, estimate) in ranked {
            if !health.get(&name).copied().unwrap_or(true) {
                continue;
            }
            if let Some(limit) = max_budget {
                if estimate.total_cost > limit {
                    continue;
                }
            }
            return Some(estimate);
        }
        None
    }
}

impl Default for CostEstimator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── helpers ──────────────────────────────────────────────────────────────

    const ONE_GIB: u64 = 1024 * 1024 * 1024;
    const ONE_DAY_SECS: u64 = 86_400;
    const THIRTY_DAYS_SECS: u64 = 30 * 86_400;

    /// A mock cost model that always returns a fixed estimate.
    struct MockModel {
        name: &'static str,
        fixed_total: f64,
        currency: &'static str,
    }

    #[async_trait]
    impl StorageCostModel for MockModel {
        fn provider_name(&self) -> &str {
            self.name
        }

        async fn estimate_cost(
            &self,
            _size_bytes: u64,
            _duration_secs: u64,
        ) -> Result<CostEstimate, StorageError> {
            Ok(CostEstimate {
                provider: self.name.to_string(),
                one_time_cost: 0.0,
                recurring_cost_per_day: 0.0,
                total_cost: self.fixed_total,
                currency: self.currency.to_string(),
            })
        }
    }

    /// A mock cost model that always fails.
    struct FailingModel;

    #[async_trait]
    impl StorageCostModel for FailingModel {
        fn provider_name(&self) -> &str {
            "failing"
        }

        async fn estimate_cost(
            &self,
            _size_bytes: u64,
            _duration_secs: u64,
        ) -> Result<CostEstimate, StorageError> {
            Err(StorageError::ProviderError("mock failure".to_string()))
        }
    }

    // ── IPFS ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_ipfs_local_is_free() {
        let model = IpfsCostModel::default(); // is_local = true
        let est = model.estimate_cost(ONE_GIB, THIRTY_DAYS_SECS).await.unwrap();
        assert_eq!(est.total_cost, 0.0);
        assert_eq!(est.one_time_cost, 0.0);
        assert_eq!(est.recurring_cost_per_day, 0.0);
        assert_eq!(est.currency, "free");
    }

    #[tokio::test]
    async fn test_ipfs_remote_1gib_30days() {
        let model = IpfsCostModel {
            is_local: false,
            remote_usd_per_gib_per_month: 0.15,
        };
        let est = model.estimate_cost(ONE_GIB, THIRTY_DAYS_SECS).await.unwrap();
        // 1 GiB × $0.15/GiB/month × 1 month = $0.15
        let expected = 0.15;
        assert!((est.total_cost - expected).abs() < 1e-9, "expected {expected}, got {}", est.total_cost);
        assert_eq!(est.currency, "USD");
    }

    #[tokio::test]
    async fn test_ipfs_remote_zero_duration() {
        let model = IpfsCostModel {
            is_local: false,
            remote_usd_per_gib_per_month: 0.15,
        };
        let est = model.estimate_cost(ONE_GIB, 0).await.unwrap();
        assert_eq!(est.recurring_cost_per_day, 0.0);
    }

    // ── Filecoin ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_filecoin_cost_in_fil() {
        let model = FilecoinCostModel {
            max_price_per_gib_epoch: 1_000_000_000, // 1e9 attoFIL/GiB/epoch
            fil_usd_rate: 0.0,
        };
        // 1 GiB, 1 day = 86400 / 30 = 2880 epochs
        let est = model.estimate_cost(ONE_GIB, ONE_DAY_SECS).await.unwrap();
        // size_gib=1, epochs=2880, price=1e9/1e18=1e-9 FIL/GiB/epoch
        // total = 1 * 1e-9 * 2880 = 2.88e-6 FIL
        let expected = 2.88e-6_f64;
        assert!((est.total_cost - expected).abs() < 1e-12, "expected {expected:.2e}, got {:.2e}", est.total_cost);
        assert_eq!(est.currency, "FIL");
    }

    #[tokio::test]
    async fn test_filecoin_cost_in_usd() {
        let model = FilecoinCostModel {
            max_price_per_gib_epoch: 1_000_000_000,
            fil_usd_rate: 5.0, // $5 per FIL
        };
        let est = model.estimate_cost(ONE_GIB, ONE_DAY_SECS).await.unwrap();
        assert_eq!(est.currency, "USD");
        assert!(est.total_cost > 0.0);
    }

    #[tokio::test]
    async fn test_filecoin_minimum_one_epoch() {
        let model = FilecoinCostModel::default();
        // 10 seconds < 1 epoch → clamp to 1 epoch
        let est = model.estimate_cost(ONE_GIB, 10).await.unwrap();
        assert!(est.total_cost > 0.0);
    }

    // ── Arweave ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_arweave_fallback_1gib() {
        let model = ArweaveCostModel::default(); // ar_per_kib = 0.0001
        // 1 GiB = 1_048_576 KiB → 104.8576 AR
        let est = model.estimate_cost(ONE_GIB, THIRTY_DAYS_SECS).await.unwrap();
        let expected = (ONE_GIB as f64 / 1024.0) * 0.0001;
        assert!((est.total_cost - expected).abs() < 1e-6);
        assert_eq!(est.one_time_cost, est.total_cost);
        assert_eq!(est.recurring_cost_per_day, 0.0);
        assert_eq!(est.currency, "AR");
    }

    #[tokio::test]
    async fn test_arweave_gateway_price_overrides_fallback() {
        let model = ArweaveCostModel {
            ar_per_kib: 0.0001,
            gateway_winston: Some(5_000_000_000_000), // 5 AR in winston
            ar_usd_rate: 0.0,
        };
        let est = model.estimate_cost(ONE_GIB, ONE_DAY_SECS).await.unwrap();
        // 5e12 / 1e12 = 5 AR
        assert!((est.total_cost - 5.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn test_arweave_usd_conversion() {
        let model = ArweaveCostModel {
            ar_per_kib: 0.0001,
            gateway_winston: Some(1_000_000_000_000), // 1 AR
            ar_usd_rate: 10.0,
        };
        let est = model.estimate_cost(ONE_GIB, ONE_DAY_SECS).await.unwrap();
        // 1 AR × $10 = $10
        assert!((est.total_cost - 10.0).abs() < 1e-9);
        assert_eq!(est.currency, "USD");
    }

    #[tokio::test]
    async fn test_arweave_duration_ignored() {
        let model = ArweaveCostModel::default();
        let est_7d = model.estimate_cost(ONE_GIB, 7 * ONE_DAY_SECS).await.unwrap();
        let est_365d = model.estimate_cost(ONE_GIB, 365 * ONE_DAY_SECS).await.unwrap();
        assert_eq!(est_7d.total_cost, est_365d.total_cost);
    }

    // ── CostEstimator — compare ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_compare_sorted_cheapest_first() {
        let mut estimator = CostEstimator::new();
        estimator.add_model(Box::new(MockModel { name: "expensive", fixed_total: 100.0, currency: "USD" }));
        estimator.add_model(Box::new(MockModel { name: "cheap", fixed_total: 1.0, currency: "USD" }));
        estimator.add_model(Box::new(MockModel { name: "mid", fixed_total: 10.0, currency: "USD" }));

        let results = estimator.compare(ONE_GIB, ONE_DAY_SECS).await;

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, "cheap");
        assert_eq!(results[1].0, "mid");
        assert_eq!(results[2].0, "expensive");
    }

    #[tokio::test]
    async fn test_compare_skips_failing_models() {
        let mut estimator = CostEstimator::new();
        estimator.add_model(Box::new(FailingModel));
        estimator.add_model(Box::new(MockModel { name: "ok", fixed_total: 5.0, currency: "USD" }));

        let results = estimator.compare(ONE_GIB, ONE_DAY_SECS).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "ok");
    }

    #[tokio::test]
    async fn test_compare_empty_estimator() {
        let estimator = CostEstimator::new();
        let results = estimator.compare(ONE_GIB, ONE_DAY_SECS).await;
        assert!(results.is_empty());
    }

    // ── CostEstimator — cheapest_provider ─────────────────────────────────────

    #[tokio::test]
    async fn test_cheapest_provider_no_constraints() {
        let mut estimator = CostEstimator::new();
        estimator.add_model(Box::new(MockModel { name: "a", fixed_total: 50.0, currency: "USD" }));
        estimator.add_model(Box::new(MockModel { name: "b", fixed_total: 5.0, currency: "USD" }));

        let result = estimator
            .cheapest_provider(ONE_GIB, ONE_DAY_SECS, &HashMap::new(), None)
            .await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().provider, "b");
    }

    #[tokio::test]
    async fn test_cheapest_provider_skips_unhealthy() {
        let mut estimator = CostEstimator::new();
        estimator.add_model(Box::new(MockModel { name: "a", fixed_total: 1.0, currency: "USD" }));
        estimator.add_model(Box::new(MockModel { name: "b", fixed_total: 10.0, currency: "USD" }));

        let mut health = HashMap::new();
        health.insert("a".to_string(), false); // cheapest is unhealthy

        let result = estimator
            .cheapest_provider(ONE_GIB, ONE_DAY_SECS, &health, None)
            .await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().provider, "b");
    }

    #[tokio::test]
    async fn test_cheapest_provider_budget_filter() {
        let mut estimator = CostEstimator::new();
        estimator.add_model(Box::new(MockModel { name: "a", fixed_total: 1.0, currency: "USD" }));
        estimator.add_model(Box::new(MockModel { name: "b", fixed_total: 10.0, currency: "USD" }));
        estimator.add_model(Box::new(MockModel { name: "c", fixed_total: 100.0, currency: "USD" }));

        // Budget of $5 — "a" ($1) fits, "b" ($10) does not
        let result = estimator
            .cheapest_provider(ONE_GIB, ONE_DAY_SECS, &HashMap::new(), Some(5.0))
            .await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().provider, "a");
    }

    #[tokio::test]
    async fn test_cheapest_provider_all_over_budget() {
        let mut estimator = CostEstimator::new();
        estimator.add_model(Box::new(MockModel { name: "x", fixed_total: 999.0, currency: "USD" }));

        let result = estimator
            .cheapest_provider(ONE_GIB, ONE_DAY_SECS, &HashMap::new(), Some(1.0))
            .await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cheapest_provider_all_unhealthy() {
        let mut estimator = CostEstimator::new();
        estimator.add_model(Box::new(MockModel { name: "x", fixed_total: 1.0, currency: "USD" }));

        let mut health = HashMap::new();
        health.insert("x".to_string(), false);

        let result = estimator
            .cheapest_provider(ONE_GIB, ONE_DAY_SECS, &health, None)
            .await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cheapest_provider_absent_from_health_map_assumed_healthy() {
        let mut estimator = CostEstimator::new();
        estimator.add_model(Box::new(MockModel { name: "unknown", fixed_total: 3.0, currency: "USD" }));

        // Provider not in health map → assumed healthy
        let result = estimator
            .cheapest_provider(ONE_GIB, ONE_DAY_SECS, &HashMap::new(), None)
            .await;

        assert!(result.is_some());
    }

    // ── Integration: real tier configs ────────────────────────────────────────

    #[tokio::test]
    async fn test_tier_cost_profiles_ordering() {
        // Hot = remote IPFS, Cold = Filecoin, Archive = Arweave
        // For a 1 MiB observation over 7 days the ordering should be:
        //   Arweave (one-time, tiny) < local IPFS (free) < Filecoin (recurring)
        // This test uses default config values from each model.

        let one_mib = 1024u64 * 1024;
        let seven_days = 7 * ONE_DAY_SECS;

        let arweave = ArweaveCostModel::default();
        let ipfs_local = IpfsCostModel::default();
        let filecoin = FilecoinCostModel {
            max_price_per_gib_epoch: 1_000_000_000,
            fil_usd_rate: 0.0,
        };

        let est_aw = arweave.estimate_cost(one_mib, seven_days).await.unwrap();
        let est_ip = ipfs_local.estimate_cost(one_mib, seven_days).await.unwrap();
        let est_fc = filecoin.estimate_cost(one_mib, seven_days).await.unwrap();

        // IPFS local is free
        assert_eq!(est_ip.total_cost, 0.0);
        // Arweave one-time is positive
        assert!(est_aw.total_cost > 0.0);
        // Filecoin has recurring cost in FIL
        assert!(est_fc.total_cost > 0.0);
    }

    #[tokio::test]
    async fn test_full_estimator_with_real_models() {
        let mut estimator = CostEstimator::new();
        estimator.add_model(Box::new(IpfsCostModel::default())); // free
        estimator.add_model(Box::new(FilecoinCostModel::default())); // FIL
        estimator.add_model(Box::new(ArweaveCostModel::default())); // AR

        let results = estimator.compare(ONE_GIB, THIRTY_DAYS_SECS).await;

        assert_eq!(results.len(), 3);
        // Local IPFS is free, so it sorts first
        assert_eq!(results[0].0, "ipfs");
        // All results are valid
        for (_, est) in &results {
            assert!(est.total_cost.is_finite());
            assert!(est.total_cost >= 0.0);
        }
    }
}
