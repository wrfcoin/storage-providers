use serde::{Deserialize, Serialize};

/// Configuration for connecting to an IPFS node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpfsConfig {
    /// URL of the IPFS HTTP API (Kubo).
    pub api_url: String,
    /// URL of the IPFS gateway for public reads.
    pub gateway_url: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Maximum number of concurrent connections.
    pub max_connections: u32,
    /// Whether to pin content by default after adding.
    pub pin_by_default: bool,
    /// Maximum number of entries in the LRU cache.
    pub cache_capacity: usize,
    /// Time-to-live for cache entries in seconds.
    pub cache_ttl_secs: u64,
    /// Bearer token for authenticating with the IPFS API.
    /// Read from `IPFS_API_TOKEN` env var by default.
    #[serde(default)]
    pub api_token: Option<String>,
}

impl Default for IpfsConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:5001".to_string(),
            gateway_url: "http://localhost:8080".to_string(),
            timeout_secs: 30,
            max_connections: 10,
            pin_by_default: true,
            cache_capacity: 10_000,
            cache_ttl_secs: 3600,
            api_token: std::env::var("IPFS_API_TOKEN").ok(),
        }
    }
}
