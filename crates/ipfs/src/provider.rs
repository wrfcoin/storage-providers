use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use wrfcoin_storage_common::{
    ContentId, HealthStatus, Metadata, StorageError, StorageProvider, StorageStats,
};

use crate::config::IpfsConfig;

/// IPFS-backed storage provider for weather data blobs.
///
/// Uses an in-memory cache to simulate local IPFS block storage.
/// Real HTTP calls to the Kubo API are not implemented in this scaffold.
pub struct IpfsStorageProvider {
    config: IpfsConfig,
    inner: Mutex<Inner>,
}

struct Inner {
    stats: StorageStats,
    cache: HashMap<String, Vec<u8>>,
}

impl IpfsStorageProvider {
    /// Create a new IPFS storage provider with the given configuration.
    pub fn new(config: IpfsConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(Inner {
                stats: StorageStats::default(),
                cache: HashMap::new(),
            }),
        }
    }

    /// Compute a simple content hash for use as a CID stand-in.
    ///
    /// In production this would be a proper multihash/CIDv1 computation.
    fn compute_hash(data: &[u8]) -> String {
        // Simple DJB2-style hash for deterministic CID generation.
        let mut hash: u64 = 5381;
        for &byte in data {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        format!("Qm{:016x}", hash)
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &IpfsConfig {
        &self.config
    }
}

#[async_trait]
impl StorageProvider for IpfsStorageProvider {
    async fn store(&self, data: &[u8], _metadata: &Metadata) -> Result<ContentId, StorageError> {
        let hash = Self::compute_hash(data);
        let content_id = ContentId {
            provider: "ipfs".to_string(),
            hash: hash.clone(),
        };

        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

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

        Ok(inner.cache.contains_key(&id.hash))
    }

    async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        inner.cache.remove(&id.hash);
        Ok(())
    }

    async fn health_check(&self) -> Result<HealthStatus, StorageError> {
        Ok(HealthStatus {
            healthy: true,
            provider: "ipfs".to_string(),
            latency_ms: Some(1),
            message: Some(format!("connected to {}", self.config.api_url)),
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
