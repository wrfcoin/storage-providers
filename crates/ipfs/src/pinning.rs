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
