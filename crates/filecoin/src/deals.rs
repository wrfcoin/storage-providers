use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Unique deal identifier (string form: `"deal-<n>"`).
pub type DealId = String;

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
    pub deal_id: DealId,
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
///
/// Internally keeps deals in a `Vec` for ordered iteration and a `HashMap`
/// secondary index that maps `DealId → Vec index` for O(1) lookup by ID.
#[derive(Debug)]
pub struct DealManager {
    deals: Vec<StorageDeal>,
    /// Secondary index: deal_id → position in `deals`.
    index: HashMap<DealId, usize>,
    next_deal_id: u64,
}

impl DealManager {
    /// Create a new deal manager.
    pub fn new() -> Self {
        Self {
            deals: Vec::new(),
            index: HashMap::new(),
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
    ) -> DealId {
        let deal_id = format!("deal-{}", self.next_deal_id);
        self.next_deal_id += 1;

        let deal = StorageDeal {
            deal_id: deal_id.clone(),
            miner,
            client,
            piece_cid: format!("piece-{data_cid}"),
            data_cid,
            size,
            price_per_epoch,
            start_epoch: 0, // Filled in when deal is accepted.
            end_epoch: duration_epochs,
            status: DealStatus::Proposed,
        };

        let vec_idx = self.deals.len();
        self.deals.push(deal);
        self.index.insert(deal_id.clone(), vec_idx);
        deal_id
    }

    /// Remove a deal by ID. Returns the removed deal, or `None` if not found.
    ///
    /// Uses swap-remove for O(1) removal, updating the index accordingly.
    pub fn remove_deal(&mut self, deal_id: &str) -> Option<StorageDeal> {
        let &pos = self.index.get(deal_id)?;
        self.index.remove(deal_id);

        let removed = self.deals.swap_remove(pos);

        // If swap moved another deal into `pos`, update its index entry.
        if pos < self.deals.len() {
            let swapped_id = self.deals[pos].deal_id.clone();
            self.index.insert(swapped_id, pos);
        }

        Some(removed)
    }

    /// Look up a deal by ID in O(1).
    pub fn get_deal(&self, deal_id: &str) -> Option<&StorageDeal> {
        let &pos = self.index.get(deal_id)?;
        self.deals.get(pos)
    }

    /// Check the status of a deal by its ID.
    pub fn check_deal_status(&self, deal_id: &str) -> Option<&DealStatus> {
        self.get_deal(deal_id).map(|d| &d.status)
    }

    /// Renew a deal by creating a new proposal with the same parameters.
    ///
    /// Only the fields needed for `propose_deal` are read from the original;
    /// no full `StorageDeal` clone is performed.
    /// Returns the new deal ID, or `None` if the original was not found.
    pub fn renew_deal(&mut self, deal_id: &str) -> Option<DealId> {
        let &pos = self.index.get(deal_id)?;
        let orig = &self.deals[pos];
        let duration = orig.end_epoch - orig.start_epoch;
        // Read only the fields that are needed; avoid cloning the entire struct.
        let miner = orig.miner.clone();
        let client = orig.client.clone();
        let data_cid = orig.data_cid.clone();
        let size = orig.size;
        let price_per_epoch = orig.price_per_epoch;
        Some(self.propose_deal(miner, client, data_cid, size, price_per_epoch, duration))
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn make_deal(dm: &mut DealManager) -> DealId {
        dm.propose_deal(
            "miner-1".to_string(),
            "client-1".to_string(),
            "QmData".to_string(),
            1024,
            100,
            1000,
        )
    }

    // ── Original tests ───────────────────────────────────────────────────────

    #[test]
    fn test_propose_deal() {
        let mut dm = DealManager::new();
        let id = dm.propose_deal(
            "miner-1".to_string(),
            "client-1".to_string(),
            "QmData".to_string(),
            1024,
            100,
            1000,
        );
        assert_eq!(id, "deal-1");
        assert_eq!(dm.list_deals().len(), 1);
        assert_eq!(dm.list_deals()[0].status, DealStatus::Proposed);
    }

    #[test]
    fn test_deal_id_auto_increment() {
        let mut dm = DealManager::new();
        let id1 = dm.propose_deal("m".into(), "c".into(), "d1".into(), 100, 10, 500);
        let id2 = dm.propose_deal("m".into(), "c".into(), "d2".into(), 200, 20, 500);
        assert_eq!(id1, "deal-1");
        assert_eq!(id2, "deal-2");
    }

    #[test]
    fn test_check_deal_status() {
        let mut dm = DealManager::new();
        let id = dm.propose_deal("m".into(), "c".into(), "d".into(), 100, 10, 500);
        assert_eq!(dm.check_deal_status(&id), Some(&DealStatus::Proposed));
        assert_eq!(dm.check_deal_status("nonexistent"), None);
    }

    #[test]
    fn test_renew_deal() {
        let mut dm = DealManager::new();
        let id = dm.propose_deal("miner".into(), "client".into(), "data".into(), 1024, 50, 1000);
        let new_id = dm.renew_deal(&id).unwrap();
        assert_eq!(new_id, "deal-2");
        assert_eq!(dm.list_deals().len(), 2);
        // Renewed deal has same params
        let renewed = dm.get_deal(&new_id).unwrap();
        assert_eq!(renewed.miner, "miner");
        assert_eq!(renewed.data_cid, "data");
    }

    #[test]
    fn test_renew_nonexistent_deal() {
        let mut dm = DealManager::new();
        assert_eq!(dm.renew_deal("nope"), None);
    }

    #[test]
    fn test_list_deals() {
        let mut dm = DealManager::new();
        assert!(dm.list_deals().is_empty());
        dm.propose_deal("m".into(), "c".into(), "d".into(), 100, 10, 500);
        assert_eq!(dm.list_deals().len(), 1);
    }

    // ── New tests: index consistency ─────────────────────────────────────────

    /// Insert a deal, renew it, then look up the original by ID via the index.
    /// Verifies the HashMap index returns the correct record after a renewal
    /// (which appends a new deal but does not mutate the original).
    #[test]
    fn test_index_consistent_after_insert_renew_lookup() {
        let mut dm = DealManager::new();
        let id = dm.propose_deal(
            "miner-A".to_string(),
            "client-A".to_string(),
            "QmABC".to_string(),
            2048,
            200,
            500,
        );

        let new_id = dm.renew_deal(&id).expect("renew should succeed");

        // Original is still findable via O(1) index lookup.
        let original = dm.get_deal(&id).expect("original deal not found");
        assert_eq!(original.deal_id, id);
        assert_eq!(original.miner, "miner-A");
        assert_eq!(original.size, 2048);

        // Renewed deal is also findable.
        let renewed = dm.get_deal(&new_id).expect("renewed deal not found");
        assert_eq!(renewed.miner, "miner-A");
        assert_eq!(renewed.data_cid, "QmABC");
        assert_eq!(renewed.size, 2048);

        // Index covers exactly 2 entries.
        assert_eq!(dm.list_deals().len(), 2);
    }

    /// Insert two deals, remove one, and verify the index stays coherent.
    #[test]
    fn test_index_consistent_after_remove() {
        let mut dm = DealManager::new();
        let id1 = dm.propose_deal("m".into(), "c".into(), "d1".into(), 100, 10, 300);
        let id2 = dm.propose_deal("m".into(), "c".into(), "d2".into(), 200, 20, 300);

        let removed = dm.remove_deal(&id1).expect("remove should return the deal");
        assert_eq!(removed.deal_id, id1);

        // Removed deal is no longer reachable through the index.
        assert!(dm.get_deal(&id1).is_none());
        assert!(dm.check_deal_status(&id1).is_none());

        // Remaining deal is still findable and correct.
        let remaining = dm.get_deal(&id2).expect("id2 not found after remove");
        assert_eq!(remaining.deal_id, id2);
        assert_eq!(remaining.data_cid, "d2");

        assert_eq!(dm.list_deals().len(), 1);
    }

    /// Renew a deal and confirm only status/expiry fields of the *new* deal
    /// differ from the original; all identity fields are copied unchanged.
    #[test]
    fn test_renew_only_changes_intended_fields() {
        let mut dm = DealManager::new();
        let id = make_deal(&mut dm);
        let orig = dm.get_deal(&id).unwrap().clone();

        let new_id = dm.renew_deal(&id).expect("renew should succeed");
        let renewed = dm.get_deal(&new_id).unwrap();

        // Identity / data fields are preserved.
        assert_eq!(renewed.miner, orig.miner);
        assert_eq!(renewed.client, orig.client);
        assert_eq!(renewed.data_cid, orig.data_cid);
        assert_eq!(renewed.size, orig.size);
        assert_eq!(renewed.price_per_epoch, orig.price_per_epoch);

        // The renewed deal starts as Proposed (status reset), not Active/Expired.
        assert_eq!(renewed.status, DealStatus::Proposed);

        // Duration is preserved: end_epoch − start_epoch equals the original.
        let orig_duration = orig.end_epoch - orig.start_epoch;
        let new_duration = renewed.end_epoch - renewed.start_epoch;
        assert_eq!(new_duration, orig_duration);

        // The original deal is untouched.
        let still_orig = dm.get_deal(&id).unwrap();
        assert_eq!(still_orig.status, orig.status);
        assert_eq!(still_orig.end_epoch, orig.end_epoch);
    }
}
