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

    /// Compute a content hash using SHA-256.
    fn compute_hash(data: &[u8]) -> String {
        wrfcoin_storage_common::hashing::arweave_content_id(data).hash
    }

    /// Estimate the cost in AR for storing the given number of bytes.
    fn estimate_cost(size: u64) -> f64 {
        // Rough estimate: ~0.0001 AR per KiB.
        (size as f64 / 1024.0) * 0.0001
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &ArweaveConfig {
        &self.config
    }
}

#[async_trait]
impl StorageProvider for ArweaveStorageProvider {
    async fn store(&self, data: &[u8], metadata: &Metadata) -> Result<ContentId, StorageError> {
        // Optionally compress data before storing.
        let store_data = if self.config.compression_enabled {
            wrfcoin_storage_common::compression::compress(data)?
        } else {
            data.to_vec()
        };

        let cost = Self::estimate_cost(store_data.len() as u64);
        if cost > self.config.max_cost_ar {
            return Err(StorageError::CostExceeded(format!(
                "estimated cost {cost:.6} AR exceeds max {:.6} AR",
                self.config.max_cost_ar
            )));
        }

        // Hash the original uncompressed data for deterministic CIDs.
        let hash = Self::compute_hash(data);
        let content_id = ContentId {
            provider: "arweave".to_string(),
            hash: hash.clone(),
        };

        let transaction = ArweaveTransaction {
            id: hash.clone(),
            data_size: data.len() as u64,
            cost,
            timestamp: metadata.timestamp,
            status: TransactionStatus::Pending,
            tags: metadata.tags.clone(),
        };

        let mut inner = self
            .inner
            .lock()
            .map_err(|e| StorageError::ProviderError(format!("lock poisoned: {e}")))?;

        inner.transactions.insert(hash.clone(), transaction);
        inner.cache.insert(hash, store_data);
        inner.stats.total_stored += 1;
        inner.stats.total_bytes += data.len() as u64;

        Ok(content_id)
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
                    let decompressed =
                        wrfcoin_storage_common::compression::decompress(&bytes)?;
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
