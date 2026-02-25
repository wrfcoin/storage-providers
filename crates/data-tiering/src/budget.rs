use std::collections::HashMap;

use wrfcoin_storage_common::DataTier;

/// Tracks storage costs across tiers and enforces budgets.
#[derive(Debug)]
pub struct StorageBudget {
    /// Maximum budget per tier.
    limits: HashMap<DataTier, f64>,
    /// Current accumulated spend per tier.
    current_spend: HashMap<DataTier, f64>,
}

impl StorageBudget {
    pub fn new() -> Self {
        Self {
            limits: HashMap::new(),
            current_spend: HashMap::new(),
        }
    }

    /// Set a budget limit for a specific tier.
    pub fn set_limit(&mut self, tier: DataTier, limit: f64) {
        self.limits.insert(tier, limit);
    }

    /// Record a cost for a tier. Returns `Err` if the budget would be exceeded.
    pub fn record_cost(&mut self, tier: DataTier, cost: f64) -> Result<(), String> {
        let current = self.current_spend.get(&tier).copied().unwrap_or(0.0);
        let new_total = current + cost;

        if let Some(&limit) = self.limits.get(&tier) {
            if new_total > limit {
                return Err(format!(
                    "budget exceeded for {tier:?}: {new_total:.4} > {limit:.4}"
                ));
            }
        }

        self.current_spend.insert(tier, new_total);
        Ok(())
    }

    /// Get remaining budget for a tier. Returns `f64::INFINITY` if no limit is set.
    pub fn remaining(&self, tier: &DataTier) -> f64 {
        let limit = self.limits.get(tier).copied().unwrap_or(f64::INFINITY);
        let spent = self.current_spend.get(tier).copied().unwrap_or(0.0);
        (limit - spent).max(0.0)
    }

    /// Total spend across all tiers.
    pub fn total_spend(&self) -> f64 {
        self.current_spend.values().sum()
    }

    /// Current spend for a specific tier.
    pub fn tier_spend(&self, tier: &DataTier) -> f64 {
        self.current_spend.get(tier).copied().unwrap_or(0.0)
    }
}

impl Default for StorageBudget {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_within_limit() {
        let mut budget = StorageBudget::new();
        budget.set_limit(DataTier::Hot, 100.0);
        assert!(budget.record_cost(DataTier::Hot, 50.0).is_ok());
        assert!(budget.record_cost(DataTier::Hot, 30.0).is_ok());
    }

    #[test]
    fn test_budget_exceeds_limit() {
        let mut budget = StorageBudget::new();
        budget.set_limit(DataTier::Hot, 100.0);
        budget.record_cost(DataTier::Hot, 80.0).unwrap();
        let result = budget.record_cost(DataTier::Hot, 30.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_budget_remaining() {
        let mut budget = StorageBudget::new();
        budget.set_limit(DataTier::Warm, 100.0);
        budget.record_cost(DataTier::Warm, 40.0).unwrap();
        assert!((budget.remaining(&DataTier::Warm) - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_budget_no_limit_set() {
        let mut budget = StorageBudget::new();
        // No limit means unlimited.
        assert!(budget.record_cost(DataTier::Cold, 999.0).is_ok());
        assert!(budget.remaining(&DataTier::Cold).is_infinite());
    }

    #[test]
    fn test_budget_total_spend() {
        let mut budget = StorageBudget::new();
        budget.record_cost(DataTier::Hot, 10.0).unwrap();
        budget.record_cost(DataTier::Cold, 20.0).unwrap();
        assert!((budget.total_spend() - 30.0).abs() < f64::EPSILON);
    }
}
