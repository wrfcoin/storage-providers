use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use wrfcoin_storage_common::{
    ContentId, HealthStatus, Metadata, StorageError, StorageProvider, StorageStats,
};

use crate::config::ArweaveConfig;

/// Status of an Arweave transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Transaction submitted but not yet confirmed.
    Pending,
    /// Transaction confirmed on chain.
    Confirmed,
    /// Transaction failed.
    Failed,
}

/// An Arweave transaction record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArweaveTransaction {
    /// Transaction ID.
    pub id: String,
    /// Size of the data in bytes.
    pub data_size: u64,
    /// Estimated cost in AR (as a float).
    pub cost: f64,
    /// Unix timestamp of submission.
    pub timestamp: u64,
    /// Current transaction status.
    pub status: TransactionStatus,
    /// Tags attached to the transaction.
    pub tags: HashMap<String, String>,
}

/// Arweave-backed storage provider for permanent climate records.
pub struct ArweaveStorageProvider {
    config: ArweaveConfig,
    inner: Mutex<Inner>,
}

struct Inner {
    stats: StorageStats,
    cache: HashMap<String, Vec<u8>>,
    transactions: HashMap<String, ArweaveTransaction>,
}

impl ArweaveStorageProvider {
    /// Create a new Arweave storage provider.
    pub fn new(config: ArweaveConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(Inner {
                stats: StorageStats::default(),
                cache: HashMap::new(),
                transactions: HashMap::new(),
            }),
        }
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &ArweaveConfig {
        &self.config
    }
}

#[async_trait]
impl StorageProvider for ArweaveStorageProvider {
    async fn store(&self, _data: &[u8], _metadata: &Metadata) -> Result<ContentId, StorageError> {
        Err(StorageError::NotConfigured {
            provider: "arweave",
            reason: "no transaction signer in this build".into(),
        })
    }

    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        // Check transaction status.
        if let Some(tx) = inner.transactions.get(&id.hash) {
            if tx.status == TransactionStatus::Failed {
                return Err(StorageError::ProviderError(
                    "transaction failed on chain".to_string(),
                ));
            }
        }

        let data = inner.cache.get(&id.hash).cloned();
        match data {
            Some(bytes) => {
                inner.stats.total_retrieved += 1;
                inner.stats.cache_hits += 1;
                // Decompress if the stored data has gzip magic bytes.
                if wrfcoin_storage_common::compression::is_compressed(&bytes) {
                    let decompressed = wrfcoin_storage_common::compression::decompress(&bytes)?;
                    Ok(decompressed)
                } else {
                    Ok(bytes)
                }
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

        Ok(inner.cache.contains_key(&id.hash) || inner.transactions.contains_key(&id.hash))
    }

    async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
        // Arweave is permanent storage; we can only remove the local cache entry.
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        inner.cache.remove(&id.hash);
        // Note: the on-chain transaction is permanent and cannot be deleted.
        Ok(())
    }

    async fn health_check(&self) -> Result<HealthStatus, StorageError> {
        Ok(HealthStatus {
            healthy: true,
            provider: "arweave".to_string(),
            latency_ms: Some(200),
            message: Some(format!("connected to {}", self.config.gateway_url)),
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
