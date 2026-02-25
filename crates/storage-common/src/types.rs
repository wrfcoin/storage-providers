use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Represents a content identifier across storage providers.
/// Maps to a CID (IPFS), transaction ID (Arweave), or deal ID (Filecoin).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ContentId {
    /// The storage provider that owns this identifier (e.g. "ipfs", "filecoin", "arweave").
    pub provider: String,
    /// The content hash or identifier string.
    pub hash: String,
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider, self.hash)
    }
}

/// Metadata associated with stored content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// MIME type of the content (e.g. "application/octet-stream").
    pub content_type: String,
    /// Size of the content in bytes.
    pub size: u64,
    /// Unix timestamp when the content was created or ingested.
    pub timestamp: u64,
    /// Arbitrary key-value tags for classification.
    pub tags: HashMap<String, String>,
}

/// Receipt returned after a successful storage operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageReceipt {
    /// The content identifier assigned by the provider.
    pub content_id: ContentId,
    /// Name of the provider that stored the data.
    pub provider: String,
    /// Unix timestamp of the storage event.
    pub timestamp: u64,
    /// Size of the stored data in bytes.
    pub size: u64,
}

/// Health status of a storage provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Whether the provider is currently healthy and reachable.
    pub healthy: bool,
    /// Name of the provider.
    pub provider: String,
    /// Round-trip latency in milliseconds, if measured.
    pub latency_ms: Option<u64>,
    /// Human-readable status message.
    pub message: Option<String>,
}

/// Aggregate statistics for a storage provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageStats {
    /// Total number of store operations completed.
    pub total_stored: u64,
    /// Total number of retrieve operations completed.
    pub total_retrieved: u64,
    /// Total bytes written across all store operations.
    pub total_bytes: u64,
    /// Number of retrieval requests served from cache.
    pub cache_hits: u64,
    /// Number of retrieval requests that missed the cache.
    pub cache_misses: u64,
}

/// Classification tier for data lifecycle management.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataTier {
    /// Frequently accessed, lowest latency (e.g. IPFS pinned).
    Hot,
    /// Moderately accessed, balanced cost/latency.
    Warm,
    /// Infrequently accessed, lower cost (e.g. Filecoin deals).
    Cold,
    /// Permanent archival, highest durability (e.g. Arweave).
    Archive,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_id_display() {
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "QmTest123".to_string(),
        };
        assert_eq!(format!("{}", cid), "ipfs:QmTest123");
    }

    #[test]
    fn test_content_id_equality() {
        let a = ContentId { provider: "ipfs".to_string(), hash: "abc".to_string() };
        let b = ContentId { provider: "ipfs".to_string(), hash: "abc".to_string() };
        let c = ContentId { provider: "ipfs".to_string(), hash: "def".to_string() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_metadata_tags() {
        let mut tags = HashMap::new();
        tags.insert("type".to_string(), "weather".to_string());
        let meta = Metadata {
            content_type: "application/json".to_string(),
            size: 1024,
            timestamp: 1000,
            tags,
        };
        assert_eq!(meta.tags.get("type").unwrap(), "weather");
        assert_eq!(meta.size, 1024);
    }

    #[test]
    fn test_storage_receipt() {
        let receipt = StorageReceipt {
            content_id: ContentId { provider: "ipfs".to_string(), hash: "Qm123".to_string() },
            provider: "ipfs".to_string(),
            timestamp: 1000,
            size: 512,
        };
        assert_eq!(receipt.size, 512);
        assert_eq!(receipt.provider, "ipfs");
    }

    #[test]
    fn test_health_status() {
        let status = HealthStatus {
            healthy: true,
            provider: "ipfs".to_string(),
            latency_ms: Some(5),
            message: None,
        };
        assert!(status.healthy);
        assert_eq!(status.latency_ms, Some(5));
    }

    #[test]
    fn test_storage_stats_default() {
        let stats = StorageStats::default();
        assert_eq!(stats.total_stored, 0);
        assert_eq!(stats.total_retrieved, 0);
        assert_eq!(stats.total_bytes, 0);
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.cache_misses, 0);
    }

    #[test]
    fn test_data_tier_variants() {
        let tiers = [DataTier::Hot, DataTier::Warm, DataTier::Cold, DataTier::Archive];
        assert_eq!(tiers.len(), 4);
        assert_ne!(DataTier::Hot, DataTier::Cold);
    }
}
