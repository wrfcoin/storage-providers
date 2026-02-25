use std::collections::HashMap;

use wrfcoin_storage_common::DataTier;

/// Tracks content access patterns to inform tiering decisions.
#[derive(Debug)]
pub struct AccessTracker {
    /// Maps content ID (as string) to a list of access timestamps.
    access_log: HashMap<String, Vec<u64>>,
}

impl AccessTracker {
    /// Create a new access tracker.
    pub fn new() -> Self {
        Self {
            access_log: HashMap::new(),
        }
    }

    /// Record an access event for the given content at the given timestamp.
    pub fn record_access(&mut self, content_id: &str, timestamp: u64) {
        self.access_log
            .entry(content_id.to_string())
            .or_default()
            .push(timestamp);
    }

    /// Return the total number of accesses for the given content.
    pub fn get_access_count(&self, content_id: &str) -> u64 {
        self.access_log
            .get(content_id)
            .map(|v| v.len() as u64)
            .unwrap_or(0)
    }

    /// Return the timestamp of the most recent access, or `None` if never accessed.
    pub fn get_last_access(&self, content_id: &str) -> Option<u64> {
        self.access_log
            .get(content_id)
            .and_then(|v| v.iter().copied().max())
    }

    /// Suggest a storage tier based on access frequency and recency.
    ///
    /// # Arguments
    /// * `content_id` - The content to evaluate.
    /// * `current_time` - The current unix timestamp.
    pub fn suggest_tier(&self, content_id: &str, current_time: u64) -> DataTier {
        let count = self.get_access_count(content_id);
        let last = self.get_last_access(content_id).unwrap_or(0);
        let age_secs = current_time.saturating_sub(last);

        // High frequency and recent -> Hot
        if count >= 10 && age_secs < 3600 {
            return DataTier::Hot;
        }

        // Moderate frequency or moderately recent -> Warm
        if count >= 3 && age_secs < 86_400 {
            return DataTier::Warm;
        }

        // Low frequency, accessed within the last week -> Cold
        if age_secs < 604_800 {
            return DataTier::Cold;
        }

        // Everything else -> Archive
        DataTier::Archive
    }

    /// Remove access records for the given content.
    pub fn clear(&mut self, content_id: &str) {
        self.access_log.remove(content_id);
    }

    /// Return the number of distinct content IDs being tracked.
    pub fn tracked_count(&self) -> usize {
        self.access_log.len()
    }
}

impl Default for AccessTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_count() {
        let mut at = AccessTracker::new();
        at.record_access("cid1", 100);
        at.record_access("cid1", 200);
        assert_eq!(at.get_access_count("cid1"), 2);
        assert_eq!(at.get_access_count("unknown"), 0);
    }

    #[test]
    fn test_last_access() {
        let mut at = AccessTracker::new();
        at.record_access("cid1", 100);
        at.record_access("cid1", 300);
        at.record_access("cid1", 200);
        assert_eq!(at.get_last_access("cid1"), Some(300));
        assert_eq!(at.get_last_access("unknown"), None);
    }

    #[test]
    fn test_suggest_tier_hot() {
        let mut at = AccessTracker::new();
        let now = 1000;
        // 10+ accesses, last within 1 hour
        for i in 0..10 {
            at.record_access("hot", now - i);
        }
        assert_eq!(at.suggest_tier("hot", now), DataTier::Hot);
    }

    #[test]
    fn test_suggest_tier_warm() {
        let mut at = AccessTracker::new();
        let now = 100_000;
        for i in 0..3 {
            at.record_access("warm", now - i * 100);
        }
        assert_eq!(at.suggest_tier("warm", now), DataTier::Warm);
    }

    #[test]
    fn test_suggest_tier_cold() {
        let mut at = AccessTracker::new();
        let now = 500_000;
        at.record_access("cold", now - 100_000); // 100k secs ago (~1 day)
        assert_eq!(at.suggest_tier("cold", now), DataTier::Cold);
    }

    #[test]
    fn test_suggest_tier_archive() {
        let mut at = AccessTracker::new();
        let now = 1_000_000;
        at.record_access("old", 1); // Very old
        assert_eq!(at.suggest_tier("old", now), DataTier::Archive);
    }

    #[test]
    fn test_suggest_tier_never_accessed() {
        let at = AccessTracker::new();
        // Never accessed: last=0, age=current_time-0=current_time
        // At time 1000: age=1000 < 604_800 (7 days), so Cold
        assert_eq!(at.suggest_tier("never", 1000), DataTier::Cold);
        // At time 700_000: age=700_000 > 604_800, so Archive
        assert_eq!(at.suggest_tier("never", 700_000), DataTier::Archive);
    }

    #[test]
    fn test_clear_and_tracked_count() {
        let mut at = AccessTracker::new();
        at.record_access("a", 1);
        at.record_access("b", 2);
        assert_eq!(at.tracked_count(), 2);
        at.clear("a");
        assert_eq!(at.tracked_count(), 1);
        assert_eq!(at.get_access_count("a"), 0);
    }
}
