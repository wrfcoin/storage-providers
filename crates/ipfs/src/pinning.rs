use serde::{Deserialize, Serialize};

/// Strategy for deciding which content to pin on the local IPFS node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PinningStrategy {
    /// Pin all content unconditionally.
    PinAll,
    /// Pin content accessed at least `min_access` times.
    FrequencyBased { min_access: u32 },
    /// Pin content up to `max_bytes` total size.
    SizeBased { max_bytes: u64 },
    /// Adaptive pinning that combines frequency, size, and recency heuristics.
    Smart,
}

/// Manages pinning decisions for IPFS content.
#[derive(Debug)]
pub struct PinManager {
    strategy: PinningStrategy,
    total_pinned_bytes: u64,
}

impl PinManager {
    /// Create a new pin manager with the given strategy.
    pub fn new(strategy: PinningStrategy) -> Self {
        Self {
            strategy,
            total_pinned_bytes: 0,
        }
    }

    /// Decide whether to pin content based on the configured strategy.
    ///
    /// # Arguments
    /// * `data_size` - Size of the content in bytes.
    /// * `access_count` - Number of times the content has been accessed.
    pub fn should_pin(&self, data_size: u64, access_count: u32) -> bool {
        match &self.strategy {
            PinningStrategy::PinAll => true,
            PinningStrategy::FrequencyBased { min_access } => access_count >= *min_access,
            PinningStrategy::SizeBased { max_bytes } => {
                self.total_pinned_bytes + data_size <= *max_bytes
            }
            PinningStrategy::Smart => {
                // Simple heuristic: pin if accessed more than twice and under 100 MiB.
                access_count > 2 && data_size < 100 * 1024 * 1024
            }
        }
    }

    /// Record that content of the given size was pinned.
    pub fn record_pin(&mut self, data_size: u64) {
        self.total_pinned_bytes += data_size;
    }

    /// Record that content of the given size was unpinned.
    pub fn record_unpin(&mut self, data_size: u64) {
        self.total_pinned_bytes = self.total_pinned_bytes.saturating_sub(data_size);
    }

    /// Return the total bytes currently pinned.
    pub fn total_pinned_bytes(&self) -> u64 {
        self.total_pinned_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pin_all_strategy() {
        let pm = PinManager::new(PinningStrategy::PinAll);
        assert!(pm.should_pin(1_000_000, 0));
        assert!(pm.should_pin(0, 0));
    }

    #[test]
    fn test_frequency_based_strategy() {
        let pm = PinManager::new(PinningStrategy::FrequencyBased { min_access: 5 });
        assert!(!pm.should_pin(100, 3));
        assert!(pm.should_pin(100, 5));
        assert!(pm.should_pin(100, 10));
    }

    #[test]
    fn test_size_based_strategy() {
        let mut pm = PinManager::new(PinningStrategy::SizeBased { max_bytes: 1000 });
        assert!(pm.should_pin(500, 0));
        pm.record_pin(800);
        assert!(!pm.should_pin(500, 0)); // 800 + 500 > 1000
        assert!(pm.should_pin(200, 0));  // 800 + 200 = 1000
    }

    #[test]
    fn test_smart_strategy() {
        let pm = PinManager::new(PinningStrategy::Smart);
        // Needs > 2 accesses AND < 100 MiB
        assert!(!pm.should_pin(100, 0)); // too few accesses
        assert!(!pm.should_pin(100, 2)); // exactly 2, need > 2
        assert!(pm.should_pin(100, 3));  // good
        assert!(!pm.should_pin(200 * 1024 * 1024, 5)); // too large
    }

    #[test]
    fn test_record_pin_unpin() {
        let mut pm = PinManager::new(PinningStrategy::PinAll);
        pm.record_pin(500);
        pm.record_pin(300);
        assert_eq!(pm.total_pinned_bytes(), 800);
        pm.record_unpin(200);
        assert_eq!(pm.total_pinned_bytes(), 600);
    }

    #[test]
    fn test_unpin_saturating() {
        let mut pm = PinManager::new(PinningStrategy::PinAll);
        pm.record_pin(100);
        pm.record_unpin(500); // More than pinned
        assert_eq!(pm.total_pinned_bytes(), 0);
    }
}
