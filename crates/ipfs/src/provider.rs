use std::sync::Mutex;

use async_trait::async_trait;
use wrfcoin_storage_common::{
    ContentId, HealthStatus, Metadata, StorageError, StorageProvider, StorageStats,
};

use crate::cache::TtlLruCache;
use crate::config::IpfsConfig;
use crate::http_client::KuboClient;

/// IPFS-backed storage provider for weather data blobs.
///
/// Uses an LRU cache as an L1 layer. When a Kubo HTTP client is available,
/// data is also stored/retrieved via the IPFS network.
pub struct IpfsStorageProvider {
    config: IpfsConfig,
    inner: Mutex<Inner>,
    http_client: Option<KuboClient>,
    require_kubo: bool,
}

struct Inner {
    stats: StorageStats,
    cache: TtlLruCache,
}

impl IpfsStorageProvider {
    /// Create a new IPFS storage provider. Attempts to build a Kubo HTTP client;
    /// falls back to pure in-memory mode if it fails.
    pub fn new(config: IpfsConfig) -> Self {
        let http_client = KuboClient::new(&config).ok();
        let cache = TtlLruCache::new(config.cache_capacity, config.cache_ttl_secs);
        Self {
            config,
            inner: Mutex::new(Inner {
                stats: StorageStats::default(),
                cache,
            }),
            http_client,
            require_kubo: false,
        }
    }

    /// Daemon mode: construction and remote writes must succeed through Kubo.
    pub fn try_new(config: IpfsConfig) -> Result<Self, StorageError> {
        let client = KuboClient::new(&config)?;
        let cache = TtlLruCache::new(config.cache_capacity, config.cache_ttl_secs);
        Ok(Self {
            config,
            inner: Mutex::new(Inner {
                stats: StorageStats::default(),
                cache,
            }),
            http_client: Some(client),
            require_kubo: true,
        })
    }

    /// Create a provider in pure in-memory mode (no HTTP client).
    /// Useful for testing.
    pub fn new_in_memory(config: IpfsConfig) -> Self {
        let cache = TtlLruCache::new(config.cache_capacity, config.cache_ttl_secs);
        Self {
            config,
            inner: Mutex::new(Inner {
                stats: StorageStats::default(),
                cache,
            }),
            http_client: None,
            require_kubo: false,
        }
    }

    /// Compute a content hash using SHA-256.
    fn compute_hash(data: &[u8]) -> String {
        wrfcoin_storage_common::hashing::ipfs_content_id(data).hash
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &IpfsConfig {
        &self.config
    }
}

#[async_trait]
impl StorageProvider for IpfsStorageProvider {
    async fn store(&self, data: &[u8], _metadata: &Metadata) -> Result<ContentId, StorageError> {
        let local_hash = Self::compute_hash(data);

        // Try HTTP upload if available. Use the real CID from Kubo when possible.
        let final_hash = if let Some(ref client) = self.http_client {
            match client.add(data.to_vec()).await {
                Ok(resp) => resp.hash,
                Err(error) if self.require_kubo => return Err(error),
                Err(_) => local_hash.clone(),
            }
        } else {
            local_hash.clone()
        };

        let content_id = ContentId {
            provider: "ipfs".to_string(),
            hash: final_hash.clone(),
        };

        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        inner.cache.insert(final_hash, data.to_vec());
        inner.stats.total_stored += 1;
        inner.stats.total_bytes += data.len() as u64;

        Ok(content_id)
    }

    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        // Check L1 cache first.
        {
            let mut inner = self
                .inner
                .lock()
                .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

            if let Some(data) = inner.cache.get(&id.hash) {
                inner.stats.total_retrieved += 1;
                inner.stats.cache_hits += 1;
                return Ok(data);
            }
        }

        // Cache miss — try HTTP if available.
        if let Some(ref client) = self.http_client {
            if let Ok(data) = client.cat(&id.hash).await {
                let mut inner = self
                    .inner
                    .lock()
                    .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;
                inner.cache.insert(id.hash.clone(), data.clone());
                inner.stats.total_retrieved += 1;
                inner.stats.cache_misses += 1;
                return Ok(data);
            }
        }

        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;
        inner.stats.cache_misses += 1;
        Err(StorageError::NotFound(id.to_string()))
    }

    async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        Ok(inner.cache.contains(&id.hash))
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
        if let Some(ref client) = self.http_client {
            match client.id().await {
                Ok(resp) => {
                    return Ok(HealthStatus {
                        healthy: true,
                        provider: "ipfs".to_string(),
                        latency_ms: Some(1),
                        message: Some(format!("kubo node {}", resp.id)),
                    });
                }
                Err(_) => {
                    return Ok(HealthStatus {
                        healthy: false,
                        provider: "ipfs".to_string(),
                        latency_ms: None,
                        message: Some("kubo node unreachable".to_string()),
                    });
                }
            }
        }

        Ok(HealthStatus {
            healthy: true,
            provider: "ipfs".to_string(),
            latency_ms: Some(1),
            message: Some(format!("in-memory mode ({})", self.config.api_url)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_metadata() -> Metadata {
        Metadata {
            content_type: "application/json".to_string(),
            size: 100,
            timestamp: 1000,
            tags: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_store_and_retrieve() {
        let provider = IpfsStorageProvider::new_in_memory(IpfsConfig::default());
        let data = b"hello weather data";
        let meta = test_metadata();
        let cid = provider.store(data, &meta).await.unwrap();
        assert_eq!(cid.provider, "ipfs");
        assert!(cid.hash.starts_with("Qm"));

        let retrieved = provider.retrieve(&cid).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_deterministic_hash() {
        let provider = IpfsStorageProvider::new_in_memory(IpfsConfig::default());
        let data = b"same data";
        let meta = test_metadata();
        let cid1 = provider.store(data, &meta).await.unwrap();
        let cid2 = provider.store(data, &meta).await.unwrap();
        assert_eq!(cid1, cid2);
    }

    #[tokio::test]
    async fn test_retrieve_not_found() {
        let provider = IpfsStorageProvider::new_in_memory(IpfsConfig::default());
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "nonexistent".to_string(),
        };
        let result = provider.retrieve(&cid).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exists() {
        let provider = IpfsStorageProvider::new_in_memory(IpfsConfig::default());
        let data = b"test";
        let cid = provider.store(data, &test_metadata()).await.unwrap();
        assert!(provider.exists(&cid).await.unwrap());

        let missing = ContentId {
            provider: "ipfs".to_string(),
            hash: "nope".to_string(),
        };
        assert!(!provider.exists(&missing).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let provider = IpfsStorageProvider::new_in_memory(IpfsConfig::default());
        let cid = provider.store(b"data", &test_metadata()).await.unwrap();
        assert!(provider.exists(&cid).await.unwrap());
        provider.delete(&cid).await.unwrap();
        assert!(!provider.exists(&cid).await.unwrap());
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let provider = IpfsStorageProvider::new_in_memory(IpfsConfig::default());
        let data = b"stats test";
        let cid = provider.store(data, &test_metadata()).await.unwrap();
        provider.retrieve(&cid).await.unwrap();
        let _ = provider
            .retrieve(&ContentId {
                provider: "ipfs".to_string(),
                hash: "miss".to_string(),
            })
            .await;

        let stats = provider.stats().await.unwrap();
        assert_eq!(stats.total_stored, 1);
        assert_eq!(stats.total_retrieved, 1);
        assert_eq!(stats.total_bytes, data.len() as u64);
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.cache_misses, 1);
    }

    #[tokio::test]
    async fn test_health_check() {
        let provider = IpfsStorageProvider::new_in_memory(IpfsConfig::default());
        let health = provider.health_check().await.unwrap();
        assert!(health.healthy);
        assert_eq!(health.provider, "ipfs");
    }
}
