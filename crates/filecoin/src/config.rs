use serde::{Deserialize, Serialize};

/// Configuration for connecting to a Filecoin Lotus node.
#[derive(Clone, Serialize, Deserialize)]
pub struct FilecoinConfig {
    /// URL of the Lotus JSON-RPC API.
    pub lotus_api_url: String,
    /// Reserved for future Lotus authentication; writes remain disabled in this build.
    #[serde(default, skip_serializing)]
    pub api_token: Option<String>,
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
            api_token: None,
            max_price_per_gib: 1_000_000_000,
            deal_duration_epochs: 518_400, // ~180 days
            verified_deals: false,
        }
    }
}

impl std::fmt::Debug for FilecoinConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilecoinConfig")
            .field("lotus_api_url", &self.lotus_api_url)
            .field("api_token", &"[REDACTED]")
            .field("wallet_address", &self.wallet_address)
            .field("max_price_per_gib", &self.max_price_per_gib)
            .field("deal_duration_epochs", &self.deal_duration_epochs)
            .field("verified_deals", &self.verified_deals)
            .finish()
    }
}
