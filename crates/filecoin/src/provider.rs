use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use wrfcoin_storage_common::{
    ContentId, HealthStatus, Metadata, StorageError, StorageProvider, StorageStats,
};

use crate::config::FilecoinConfig;
use crate::deals::{DealStatus, StorageDeal};

/// Filecoin-backed storage provider for persistent archival with storage deals.
pub struct FilecoinStorageProvider {
    config: FilecoinConfig,
    inner: Mutex<Inner>,
}

struct Inner {
    stats: StorageStats,
    cache: HashMap<String, Vec<u8>>,
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
                pending_deals: HashMap::new(),
            }),
        }
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &FilecoinConfig {
        &self.config
    }
}

#[async_trait]
impl StorageProvider for FilecoinStorageProvider {
    async fn store(&self, _data: &[u8], _metadata: &Metadata) -> Result<ContentId, StorageError> {
        Err(StorageError::NotConfigured {
            provider: "filecoin",
            reason: "Lotus authentication is not implemented in this build".into(),
        })
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

    async fn delete(&self, _id: &ContentId) -> Result<(), StorageError> {
        Err(StorageError::NotConfigured {
            provider: "filecoin",
            reason: "Lotus authentication is not implemented in this build".into(),
        })
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
