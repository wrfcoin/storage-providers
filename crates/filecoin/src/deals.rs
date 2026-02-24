use serde::{Deserialize, Serialize};

/// Status of a Filecoin storage deal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DealStatus {
    /// Deal has been proposed but not yet accepted by the miner.
    Proposed,
    /// Deal is active and data is being stored.
    Active,
    /// Deal has expired after reaching its end epoch.
    Expired,
    /// Miner was slashed for failing to prove storage.
    Slashed,
}

/// Represents a Filecoin storage deal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageDeal {
    /// Unique identifier for this deal.
    pub deal_id: String,
    /// Miner (storage provider) address.
    pub miner: String,
    /// Client address.
    pub client: String,
    /// CID of the piece (after padding/alignment).
    pub piece_cid: String,
    /// CID of the original data.
    pub data_cid: String,
    /// Size of the deal in bytes.
    pub size: u64,
    /// Price per epoch in attoFIL.
    pub price_per_epoch: u64,
    /// Epoch at which the deal becomes active.
    pub start_epoch: u64,
    /// Epoch at which the deal expires.
    pub end_epoch: u64,
    /// Current status of the deal.
    pub status: DealStatus,
}

/// Manages the lifecycle of Filecoin storage deals.
#[derive(Debug)]
pub struct DealManager {
    deals: Vec<StorageDeal>,
    next_deal_id: u64,
}

impl DealManager {
    /// Create a new deal manager.
    pub fn new() -> Self {
        Self {
            deals: Vec::new(),
            next_deal_id: 1,
        }
    }

    /// Propose a new storage deal. Returns the deal ID.
    pub fn propose_deal(
        &mut self,
        miner: String,
        client: String,
        data_cid: String,
        size: u64,
        price_per_epoch: u64,
        duration_epochs: u64,
    ) -> String {
        let deal_id = format!("deal-{}", self.next_deal_id);
        self.next_deal_id += 1;

        let deal = StorageDeal {
            deal_id: deal_id.clone(),
            miner,
            client,
            piece_cid: format!("piece-{}", data_cid),
            data_cid,
            size,
            price_per_epoch,
            start_epoch: 0, // Filled in when deal is accepted.
            end_epoch: duration_epochs,
            status: DealStatus::Proposed,
        };

        self.deals.push(deal);
        deal_id
    }

    /// Check the status of a deal by its ID.
    pub fn check_deal_status(&self, deal_id: &str) -> Option<&DealStatus> {
        self.deals
            .iter()
            .find(|d| d.deal_id == deal_id)
            .map(|d| &d.status)
    }

    /// Renew a deal by creating a new proposal with the same parameters.
    /// Returns the new deal ID, or `None` if the original was not found.
    pub fn renew_deal(&mut self, deal_id: &str) -> Option<String> {
        let original = self.deals.iter().find(|d| d.deal_id == deal_id)?.clone();
        let duration = original.end_epoch - original.start_epoch;
        Some(self.propose_deal(
            original.miner,
            original.client,
            original.data_cid,
            original.size,
            original.price_per_epoch,
            duration,
        ))
    }

    /// List all tracked deals.
    pub fn list_deals(&self) -> &[StorageDeal] {
        &self.deals
    }
}

impl Default for DealManager {
    fn default() -> Self {
        Self::new()
    }
}
