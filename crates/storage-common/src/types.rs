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

/// Signed provenance step binding an artifact hash to its parent lineage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceLink {
    /// Artifact hash for the current step.
    pub artifact_hash: String,
    /// Parent artifact hash (if present).
    pub parent_hash: Option<String>,
    /// Signer identity for this link.
    pub signer: String,
    /// Signature over the link payload.
    pub signature: String,
    /// Signature algorithm descriptor.
    pub algorithm: String,
    /// Unix timestamp of signature creation.
    pub signed_at: u64,
}

/// Provenance chain attached to incoming artifacts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceChain {
    /// Stable chain identifier.
    pub chain_id: String,
    /// Root hash expected at the chain head.
    pub root_hash: String,
    /// Ordered links from root -> artifact.
    pub links: Vec<ProvenanceLink>,
}

/// Result of provenance verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceValidationResult {
    pub valid: bool,
    pub reason: String,
    pub invalid_links: usize,
}

/// Routing decision used by ingestion logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AdmissionDecision {
    Accepted,
    Quarantined,
    Rejected,
}

/// Quarantine metadata for suspicious/synthetic content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuarantineRecord {
    pub quarantine_id: String,
    pub reason: String,
    pub detected_at: u64,
    pub review_required: bool,
    pub reinstated: bool,
}

/// Availability policy for public-interest report classes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicInterestAvailabilityPolicy {
    pub class: String,
    pub min_replicas: u8,
}

/// SLA status for public-interest availability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AvailabilitySlaStatus {
    pub class: String,
    pub target_replicas: u8,
    pub replicated_replicas: u8,
    pub read_failover_ready: bool,
    pub sla_met: bool,
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

    #[test]
    fn test_provenance_chain_roundtrip() {
        let chain = ProvenanceChain {
            chain_id: "chain-a".to_string(),
            root_hash: "abc".to_string(),
            links: vec![ProvenanceLink {
                artifact_hash: "abc".to_string(),
                parent_hash: None,
                signer: "signer-1".to_string(),
                signature: "sig".to_string(),
                algorithm: "ed25519".to_string(),
                signed_at: 123,
            }],
        };
        let json = serde_json::to_string(&chain).expect("serialize chain");
        let decoded: ProvenanceChain = serde_json::from_str(&json).expect("deserialize chain");
        assert_eq!(decoded.chain_id, "chain-a");
        assert_eq!(decoded.links.len(), 1);
    }

    #[test]
    fn test_availability_sla_status() {
        let status = AvailabilitySlaStatus {
            class: "public_interest.weather".to_string(),
            target_replicas: 3,
            replicated_replicas: 2,
            read_failover_ready: true,
            sla_met: false,
        };
        assert_eq!(status.class, "public_interest.weather");
        assert!(!status.sla_met);
    }
}
