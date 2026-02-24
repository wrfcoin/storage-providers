use serde::{Deserialize, Serialize};

/// Configuration for connecting to a Filecoin Lotus node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilecoinConfig {
    /// URL of the Lotus JSON-RPC API.
    pub lotus_api_url: String,
    /// Filecoin wallet address used for deal payments.
    pub wallet_address: Option<String>,
    /// Maximum price in attoFIL per GiB per epoch.
    pub max_price_per_gib: u64,
    /// Duration of storage deals in Filecoin epochs.
    pub deal_duration_epochs: u64,
    /// Whether to propose verified (DataCap) deals.
    pub verified_deals: bool,
}

impl Default for FilecoinConfig {
    fn default() -> Self {
        Self {
            lotus_api_url: "http://localhost:1234/rpc/v0".to_string(),
            wallet_address: None,
            max_price_per_gib: 1_000_000_000,
            deal_duration_epochs: 518_400, // ~180 days
            verified_deals: false,
        }
    }
}
