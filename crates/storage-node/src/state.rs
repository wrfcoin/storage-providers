use std::collections::HashMap;
use std::env;
use wrfcoin_storage_common::error::StorageError;
use wrfcoin_storage_common::traits::StorageProvider;
use wrfcoin_ipfs::{IpfsConfig, IpfsStorageProvider};
use wrfcoin_filecoin::{FilecoinConfig, FilecoinStorageProvider};
use wrfcoin_arweave::{ArweaveConfig, ArweaveStorageProvider};

/// Application state holding configured storage providers.
pub struct AppState {
    /// Map of provider names to their trait objects
    providers: HashMap<String, Box<dyn StorageProvider>>,
    /// Default provider to use if none is specified
    default_provider: String,
}

impl AppState {
    /// Initialize AppState with configured storage providers.
    ///
    /// Reads environment variables to determine which providers to initialize:
    /// - `STORAGE_PROVIDERS`: Comma-separated list of providers to enable (e.g. "ipfs,filecoin")
    /// - `DEFAULT_PROVIDER`: Default provider for operations (defaults to "ipfs")
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

        tracing::info!(
            "Initialized storage daemon with providers: {:?}",
            providers.keys().collect::<Vec<_>>()
        );

        Ok(Self {
            providers,
            default_provider,
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
}
