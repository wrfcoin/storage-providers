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
