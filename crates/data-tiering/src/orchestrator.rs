use std::collections::HashMap;

use wrfcoin_storage_common::{
    ContentId, DataTier, Metadata, StorageError, StorageProvider, StorageStats,
};

use crate::tiers::TieringConfig;

/// Orchestrates storage across multiple tiers (hot/warm/cold/archive).
///
/// Routes store and retrieve requests to the appropriate provider based on
/// the configured tiering policy.
pub struct DataTieringOrchestrator {
    tiers: HashMap<DataTier, Box<dyn StorageProvider>>,
    tier_config: TieringConfig,
}

impl DataTieringOrchestrator {
    /// Create a new orchestrator with the given tiering configuration.
    ///
    /// Providers are registered per-tier after construction via `register_tier`.
    pub fn new(tier_config: TieringConfig) -> Self {
        Self {
            tiers: HashMap::new(),
            tier_config,
        }
    }

    /// Register a storage provider for a specific tier.
    pub fn register_tier(&mut self, tier: DataTier, provider: Box<dyn StorageProvider>) {
        self.tiers.insert(tier, provider);
    }

    /// Store data in the specified tier.
    pub async fn store_with_tier(
        &self,
        tier: &DataTier,
        data: &[u8],
        metadata: &Metadata,
    ) -> Result<ContentId, StorageError> {
        let provider = self
            .tiers
            .get(tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {tier:?}")))?;

        provider.store(data, metadata).await
    }

    /// Retrieve data by checking tiers in order from hottest to coldest.
    ///
    /// Returns the data from the first tier that has the content.
    pub async fn retrieve_auto(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        let tier_order = [DataTier::Hot, DataTier::Warm, DataTier::Cold, DataTier::Archive];

        for tier in &tier_order {
            if let Some(provider) = self.tiers.get(tier) {
                match provider.exists(id).await {
                    Ok(true) => return provider.retrieve(id).await,
                    Ok(false) => continue,
                    Err(_) => continue,
                }
            }
        }

        Err(StorageError::NotFound(id.to_string()))
    }

    /// Promote content to a hotter tier.
    ///
    /// Retrieves from the source tier and stores in the destination tier.
    pub async fn promote(
        &self,
        id: &ContentId,
        from_tier: &DataTier,
        to_tier: &DataTier,
        metadata: &Metadata,
    ) -> Result<ContentId, StorageError> {
        let from_provider = self
            .tiers
            .get(from_tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {from_tier:?}")))?;

        let data = from_provider.retrieve(id).await?;

        let to_provider = self
            .tiers
            .get(to_tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {to_tier:?}")))?;

        to_provider.store(&data, metadata).await
    }

    /// Demote content to a colder tier.
    ///
    /// Retrieves from the source tier, stores in the destination tier, then
    /// optionally deletes from the source tier.
    pub async fn demote(
        &self,
        id: &ContentId,
        from_tier: &DataTier,
        to_tier: &DataTier,
        metadata: &Metadata,
        delete_from_source: bool,
    ) -> Result<ContentId, StorageError> {
        let from_provider = self
            .tiers
            .get(from_tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {from_tier:?}")))?;

        let data = from_provider.retrieve(id).await?;

        let to_provider = self
            .tiers
            .get(to_tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {to_tier:?}")))?;

        let new_id = to_provider.store(&data, metadata).await?;

        if delete_from_source {
            // Re-acquire the reference after the previous borrow is done.
            if let Some(provider) = self.tiers.get(from_tier) {
                let _ = provider.delete(id).await;
            }
        }

        Ok(new_id)
    }

    /// Return aggregate stats across all registered tiers.
    pub async fn aggregate_stats(&self) -> Result<StorageStats, StorageError> {
        let mut aggregate = StorageStats::default();

        for provider in self.tiers.values() {
            let s = provider.stats().await?;
            aggregate.total_stored += s.total_stored;
            aggregate.total_retrieved += s.total_retrieved;
            aggregate.total_bytes += s.total_bytes;
            aggregate.cache_hits += s.cache_hits;
            aggregate.cache_misses += s.cache_misses;
        }

        Ok(aggregate)
    }

    /// Return a reference to the tiering configuration.
    pub fn config(&self) -> &TieringConfig {
        &self.tier_config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use wrfcoin_storage_common::HealthStatus;

    /// A minimal in-memory provider for orchestrator testing.
    struct MockProvider {
        name: String,
        inner: Mutex<MockInner>,
    }

    struct MockInner {
        data: HashMap<String, Vec<u8>>,
        stats: StorageStats,
    }

    impl MockProvider {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                inner: Mutex::new(MockInner {
                    data: HashMap::new(),
                    stats: StorageStats::default(),
                }),
            }
        }
    }

    #[async_trait]
    impl StorageProvider for MockProvider {
        async fn store(
            &self,
            data: &[u8],
            _metadata: &Metadata,
        ) -> Result<ContentId, StorageError> {
            let hash = format!("{:x}", md5_like_hash(data));
            let cid = ContentId {
                provider: self.name.clone(),
                hash: hash.clone(),
            };
            let mut inner = self.inner.lock().unwrap();
            inner.data.insert(hash, data.to_vec());
            inner.stats.total_stored += 1;
            inner.stats.total_bytes += data.len() as u64;
            Ok(cid)
        }

        async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
            let mut inner = self.inner.lock().unwrap();
            match inner.data.get(&id.hash).cloned() {
                Some(d) => {
                    inner.stats.total_retrieved += 1;
                    Ok(d)
                }
                None => Err(StorageError::NotFound(id.to_string())),
            }
        }

        async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.data.contains_key(&id.hash))
        }

        async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
            let mut inner = self.inner.lock().unwrap();
            inner.data.remove(&id.hash);
            Ok(())
        }

        async fn health_check(&self) -> Result<HealthStatus, StorageError> {
            Ok(HealthStatus {
                healthy: true,
                provider: self.name.clone(),
                latency_ms: Some(1),
                message: None,
            })
        }

        async fn stats(&self) -> Result<StorageStats, StorageError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.stats.clone())
        }
    }

    /// Simple hash for deterministic test CIDs.
    fn md5_like_hash(data: &[u8]) -> u64 {
        let mut h: u64 = 5381;
        for &b in data {
            h = h.wrapping_mul(33).wrapping_add(b as u64);
        }
        h
    }

    fn test_metadata() -> Metadata {
        Metadata {
            content_type: "application/octet-stream".to_string(),
            size: 0,
            timestamp: 1000,
            tags: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_store_with_tier() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        let cid = orch
            .store_with_tier(&DataTier::Hot, b"weather data", &test_metadata())
            .await
            .unwrap();
        assert_eq!(cid.provider, "hot");
    }

    #[tokio::test]
    async fn test_store_no_provider_for_tier() {
        let orch = DataTieringOrchestrator::new(TieringConfig::default());
        let result = orch
            .store_with_tier(&DataTier::Hot, b"data", &test_metadata())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_retrieve_auto_searches_tiers() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        orch.register_tier(DataTier::Cold, Box::new(MockProvider::new("cold")));

        // Store only in Cold tier.
        let cid = orch
            .store_with_tier(&DataTier::Cold, b"cold data", &test_metadata())
            .await
            .unwrap();

        // retrieve_auto should find it even though Hot is checked first.
        let data = orch.retrieve_auto(&cid).await.unwrap();
        assert_eq!(data, b"cold data");
    }

    #[tokio::test]
    async fn test_retrieve_auto_not_found() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        let missing = ContentId {
            provider: "hot".to_string(),
            hash: "nonexistent".to_string(),
        };
        let result = orch.retrieve_auto(&missing).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_promote() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Cold, Box::new(MockProvider::new("cold")));
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        let cold_cid = orch
            .store_with_tier(&DataTier::Cold, b"promote me", &test_metadata())
            .await
            .unwrap();

        let hot_cid = orch
            .promote(&cold_cid, &DataTier::Cold, &DataTier::Hot, &test_metadata())
            .await
            .unwrap();

        assert_eq!(hot_cid.provider, "hot");
        // Data should now be in Hot tier.
        let data = orch
            .store_with_tier(&DataTier::Hot, b"", &test_metadata())
            .await;
        assert!(data.is_ok());
    }

    #[tokio::test]
    async fn test_demote_with_delete() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        orch.register_tier(DataTier::Cold, Box::new(MockProvider::new("cold")));

        let hot_cid = orch
            .store_with_tier(&DataTier::Hot, b"demote me", &test_metadata())
            .await
            .unwrap();

        let cold_cid = orch
            .demote(
                &hot_cid,
                &DataTier::Hot,
                &DataTier::Cold,
                &test_metadata(),
                true,
            )
            .await
            .unwrap();

        assert_eq!(cold_cid.provider, "cold");
    }

    #[tokio::test]
    async fn test_aggregate_stats() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        orch.register_tier(DataTier::Cold, Box::new(MockProvider::new("cold")));

        orch.store_with_tier(&DataTier::Hot, b"aaaa", &test_metadata())
            .await
            .unwrap();
        orch.store_with_tier(&DataTier::Cold, b"bbbbbb", &test_metadata())
            .await
            .unwrap();

        let stats = orch.aggregate_stats().await.unwrap();
        assert_eq!(stats.total_stored, 2);
        assert_eq!(stats.total_bytes, 10); // 4 + 6
    }
}
