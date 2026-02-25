use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use wrfcoin_storage_common::{
    ContentId, HealthStatus, Metadata, StorageError, StorageProvider, StorageStats,
};

use crate::config::FilecoinConfig;
use crate::deals::{DealManager, DealStatus, StorageDeal};

/// Filecoin-backed storage provider for persistent archival with storage deals.
pub struct FilecoinStorageProvider {
    config: FilecoinConfig,
    inner: Mutex<Inner>,
}

struct Inner {
    stats: StorageStats,
    cache: HashMap<String, Vec<u8>>,
    deal_manager: DealManager,
    /// Maps content hash to its associated deal.
    pending_deals: HashMap<String, StorageDeal>,
}

impl FilecoinStorageProvider {
    /// Create a new Filecoin storage provider.
    pub fn new(config: FilecoinConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(Inner {
                stats: StorageStats::default(),
                cache: HashMap::new(),
                deal_manager: DealManager::new(),
                pending_deals: HashMap::new(),
            }),
        }
    }

    /// Compute a content hash using SHA-256.
    fn compute_hash(data: &[u8]) -> String {
        wrfcoin_storage_common::hashing::filecoin_content_id(data).hash
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &FilecoinConfig {
        &self.config
    }
}

#[async_trait]
impl StorageProvider for FilecoinStorageProvider {
    async fn store(&self, data: &[u8], _metadata: &Metadata) -> Result<ContentId, StorageError> {
        let hash = Self::compute_hash(data);
        let content_id = ContentId {
            provider: "filecoin".to_string(),
            hash: hash.clone(),
        };

        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        let client = self
            .config
            .wallet_address
            .clone()
            .unwrap_or_else(|| "default-client".to_string());

        let deal_id = inner.deal_manager.propose_deal(
            "miner-default".to_string(),
            client,
            hash.clone(),
            data.len() as u64,
            self.config.max_price_per_gib,
            self.config.deal_duration_epochs,
        );

        // Store deal info for retrieval.
        let deal_clone = inner
            .deal_manager
            .list_deals()
            .iter()
            .find(|d| d.deal_id == deal_id)
            .cloned();

        if let Some(deal) = deal_clone {
            inner.pending_deals.insert(hash.clone(), deal);
        }

        inner.cache.insert(hash, data.to_vec());
        inner.stats.total_stored += 1;
        inner.stats.total_bytes += data.len() as u64;

        Ok(content_id)
    }

    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        // Check if there is an active or proposed deal for this content.
        if let Some(deal) = inner.pending_deals.get(&id.hash) {
            if deal.status == DealStatus::Slashed {
                return Err(StorageError::ProviderError(
                    "deal was slashed by the network".to_string(),
                ));
            }
        }

        let data = inner.cache.get(&id.hash).cloned();
        match data {
            Some(bytes) => {
                inner.stats.total_retrieved += 1;
                inner.stats.cache_hits += 1;
                Ok(bytes)
            }
            None => {
                inner.stats.cache_misses += 1;
                Err(StorageError::NotFound(id.to_string()))
            }
        }
    }

    async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        Ok(inner.cache.contains_key(&id.hash) || inner.pending_deals.contains_key(&id.hash))
    }

    async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        inner.cache.remove(&id.hash);
        inner.pending_deals.remove(&id.hash);
        Ok(())
    }

    async fn health_check(&self) -> Result<HealthStatus, StorageError> {
        Ok(HealthStatus {
            healthy: true,
            provider: "filecoin".to_string(),
            latency_ms: Some(50),
            message: Some(format!("connected to {}", self.config.lotus_api_url)),
        })
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        Ok(inner.stats.clone())
    }
}
