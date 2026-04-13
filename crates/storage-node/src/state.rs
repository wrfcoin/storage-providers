use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use wrfcoin_storage_common::error::StorageError;
use wrfcoin_storage_common::traits::StorageProvider;
use wrfcoin_storage_common::{DomainContentIndex, FailoverMetrics, StorageStats};
use wrfcoin_ipfs::{IpfsConfig, IpfsStorageProvider};
use wrfcoin_filecoin::{FilecoinConfig, FilecoinStorageProvider};
use wrfcoin_arweave::{ArweaveConfig, ArweaveStorageProvider};
use wrfcoin_data_tiering::{
    ArweaveCostModel, CostEstimator, FilecoinCostModel, InMemoryMetrics, IpfsCostModel,
    StorageAnalytics, StorageBudget, TieringConfig,
};

/// Application state holding configured storage providers plus cost-estimator
/// and analytics sub-systems.
pub struct AppState {
    /// Map of provider names to their trait objects.
    providers: HashMap<String, Box<dyn StorageProvider>>,
    /// Default provider to use if none is specified.
    default_provider: String,
    /// Cost estimator (compares all registered providers, cheapest-first routing).
    pub cost_estimator: CostEstimator,
    /// Storage analytics ring-buffer — protected by Mutex because `snapshot()` takes `&mut self`.
    pub analytics: Arc<Mutex<StorageAnalytics>>,
    /// Tiering configuration (cost/GiB, max sizes per tier).
    pub tiering_config: TieringConfig,
    /// Domain content index — updated by the store handler.
    pub domain_index: Arc<Mutex<DomainContentIndex>>,
    /// Budget tracking — updated when store costs are known.
    pub budget: Arc<Mutex<StorageBudget>>,
    /// Failover layer metrics (defaults; not driven by HTTP handlers).
    pub failover_metrics: Arc<Mutex<FailoverMetrics>>,
    /// Per-provider in-memory atomic metrics (updated by store/retrieve handlers).
    pub provider_metrics: Arc<HashMap<String, Arc<InMemoryMetrics>>>,
    /// Per-provider byte/operation counters (updated by store/retrieve handlers).
    pub provider_stats: Arc<Mutex<HashMap<String, StorageStats>>>,
}

impl AppState {
    /// Initialize AppState with configured storage providers.
    ///
    /// Reads environment variables to determine which providers to initialize:
    /// - `STORAGE_PROVIDERS`: Comma-separated list of providers to enable (e.g. "ipfs,filecoin")
    /// - `DEFAULT_PROVIDER`: Default provider for operations (defaults to "ipfs")
    /// - `IPFS_IS_LOCAL`: `"true"` (default) for free local node, `"false"` for remote pinning
    /// - `IPFS_REMOTE_USD_PER_GIB_MONTH`: remote IPFS cost (default: 0.15)
    /// - `FIL_USD_RATE`: FIL/USD rate for Filecoin; 0.0 → report costs in FIL (default: 0.0)
    /// - `AR_USD_RATE`: AR/USD rate for Arweave; 0.0 → report costs in AR (default: 0.0)
    pub async fn new() -> Result<Self, StorageError> {
        let mut providers: HashMap<String, Box<dyn StorageProvider>> = HashMap::new();

        let providers_list = env::var("STORAGE_PROVIDERS")
            .unwrap_or_else(|_| "ipfs".to_string());

        let default_provider = env::var("DEFAULT_PROVIDER")
            .unwrap_or_else(|_| "ipfs".to_string());

        for provider_name in providers_list.split(',') {
            let provider_name = provider_name.trim();
            match provider_name {
                "ipfs" => {
                    let config = IpfsConfig::default();
                    let ipfs_provider = IpfsStorageProvider::new(config);
                    providers.insert(provider_name.to_string(), Box::new(ipfs_provider) as Box<dyn StorageProvider>);
                }
                "filecoin" => {
                    let config = FilecoinConfig::default();
                    let fc_provider = FilecoinStorageProvider::new(config);
                    providers.insert(provider_name.to_string(), Box::new(fc_provider) as Box<dyn StorageProvider>);
                }
                "arweave" => {
                    let config = ArweaveConfig::default();
                    let ar_provider = ArweaveStorageProvider::new(config);
                    providers.insert(provider_name.to_string(), Box::new(ar_provider) as Box<dyn StorageProvider>);
                }
                other => {
                    return Err(StorageError::ConfigError(format!(
                        "Unknown provider: {}",
                        other
                    )))
                }
            }
        }

        if providers.is_empty() {
            return Err(StorageError::ConfigError(
                "No storage providers configured".to_string(),
            ));
        }

        if !providers.contains_key(&default_provider) {
            return Err(StorageError::ConfigError(format!(
                "Default provider '{}' not in configured providers",
                default_provider
            )));
        }

        // Build cost estimator for configured providers.
        let ipfs_is_local = env::var("IPFS_IS_LOCAL")
            .map(|v| v != "false")
            .unwrap_or(true);
        let ipfs_remote_rate = env::var("IPFS_REMOTE_USD_PER_GIB_MONTH")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.15);
        let fil_usd_rate = env::var("FIL_USD_RATE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let ar_usd_rate = env::var("AR_USD_RATE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);

        let mut cost_estimator = CostEstimator::new();
        let mut provider_metrics_map: HashMap<String, Arc<InMemoryMetrics>> = HashMap::new();
        let mut provider_stats_map: HashMap<String, StorageStats> = HashMap::new();

        for name in providers.keys() {
            provider_metrics_map.insert(name.clone(), Arc::new(InMemoryMetrics::new()));
            provider_stats_map.insert(name.clone(), StorageStats::default());
            match name.as_str() {
                "ipfs" => {
                    cost_estimator.add_model(Box::new(IpfsCostModel {
                        is_local: ipfs_is_local,
                        remote_usd_per_gib_per_month: ipfs_remote_rate,
                    }));
                }
                "filecoin" => {
                    cost_estimator.add_model(Box::new(FilecoinCostModel {
                        fil_usd_rate,
                        ..Default::default()
                    }));
                }
                "arweave" => {
                    cost_estimator.add_model(Box::new(ArweaveCostModel {
                        ar_usd_rate,
                        ..Default::default()
                    }));
                }
                _ => {}
            }
        }

        tracing::info!(
            "Initialized storage daemon with providers: {:?}",
            providers.keys().collect::<Vec<_>>()
        );

        Ok(Self {
            providers,
            default_provider,
            cost_estimator,
            analytics: Arc::new(Mutex::new(StorageAnalytics::new(60))),
            tiering_config: TieringConfig::default(),
            domain_index: Arc::new(Mutex::new(DomainContentIndex::new())),
            budget: Arc::new(Mutex::new(StorageBudget::new())),
            failover_metrics: Arc::new(Mutex::new(FailoverMetrics::default())),
            provider_metrics: Arc::new(provider_metrics_map),
            provider_stats: Arc::new(Mutex::new(provider_stats_map)),
        })
    }

    /// Get a specific provider by name.
    pub fn get_provider(&self, name: &str) -> Result<&dyn StorageProvider, StorageError> {
        self.providers
            .get(name)
            .map(|b| b.as_ref())
            .ok_or_else(|| StorageError::ConfigError(format!("Provider '{}' not found", name)))
    }

    /// Get the default provider name.
    pub fn default_provider_name(&self) -> &str {
        &self.default_provider
    }

    /// Get all available provider names.
    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.keys().map(|k| k.as_str()).collect()
    }

    /// Get in-memory metrics for a named provider, if tracked.
    #[allow(dead_code)]
    pub fn get_metrics(&self, name: &str) -> Option<Arc<InMemoryMetrics>> {
        self.provider_metrics.get(name).cloned()
    }
}
