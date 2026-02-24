use serde::{Deserialize, Serialize};

/// Configuration for connecting to an Arweave gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArweaveConfig {
    /// URL of the Arweave gateway.
    pub gateway_url: String,
    /// Path to the JWK wallet file for signing transactions.
    pub wallet_path: Option<String>,
    /// Maximum cost in AR tokens per transaction.
    pub max_cost_ar: f64,
    /// Whether to compress data before uploading.
    pub compression_enabled: bool,
}

impl Default for ArweaveConfig {
    fn default() -> Self {
        Self {
            gateway_url: "https://arweave.net".to_string(),
            wallet_path: None,
            max_cost_ar: 1.0,
            compression_enabled: true,
        }
    }
}
