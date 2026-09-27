use serde::{Deserialize, Serialize};

/// Strategy for bundling multiple data items into a single Arweave transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BundlingStrategy {
    /// No bundling; each item is uploaded individually.
    None,
    /// Bundle when accumulated data reaches the byte threshold.
    SizeThreshold { bytes: u64 },
    /// Bundle after a time window elapses (in seconds).
    TimeWindow { secs: u64 },
    /// Adaptive strategy that considers both size and time.
    Smart,
}

/// A single item waiting to be bundled.
#[derive(Debug, Clone)]
pub struct BundleItem {
    /// Identifier for tracking this item.
    pub id: String,
    /// Raw data bytes.
    pub data: Vec<u8>,
    /// Unix timestamp when the item was added.
    pub added_at: u64,
}

/// Manages accumulation and flushing of data bundles for Arweave.
#[derive(Debug)]
pub struct DataBundler {
    pending_items: Vec<BundleItem>,
    strategy: BundlingStrategy,
}

impl DataBundler {
    /// Create a new data bundler with the given strategy.
    pub fn new(strategy: BundlingStrategy) -> Self {
        Self {
            pending_items: Vec::new(),
            strategy,
        }
    }

    /// Add an item to the pending bundle.
    pub fn add_item(&mut self, item: BundleItem) {
        self.pending_items.push(item);
    }

    /// Return the number of pending items.
    pub fn pending_count(&self) -> usize {
        self.pending_items.len()
    }

    /// Check whether the bundle should be flushed based on the current strategy.
    ///
    /// # Arguments
    /// * `current_time` - The current unix timestamp (for time-based strategies).
    pub fn should_flush(&self, current_time: u64) -> bool {
        if self.pending_items.is_empty() {
            return false;
        }

        match &self.strategy {
            BundlingStrategy::None => true,
            BundlingStrategy::SizeThreshold { bytes } => {
                let total: u64 = self.pending_items.iter().map(|i| i.data.len() as u64).sum();
                total >= *bytes
            }
            BundlingStrategy::TimeWindow { secs } => {
                if let Some(first) = self.pending_items.first() {
                    current_time.saturating_sub(first.added_at) >= *secs
                } else {
                    false
                }
            }
            BundlingStrategy::Smart => {
                let total_bytes: u64 = self.pending_items.iter().map(|i| i.data.len() as u64).sum();
                let oldest = self
                    .pending_items
                    .first()
                    .map(|i| i.added_at)
                    .unwrap_or(current_time);
                let age = current_time.saturating_sub(oldest);

                // Flush if over 1 MiB or older than 5 minutes, or 10+ items.
                total_bytes >= 1_048_576 || age >= 300 || self.pending_items.len() >= 10
            }
        }
    }

    /// Flush all pending items, returning the concatenated bundle data.
    ///
    /// The caller is responsible for uploading the returned bytes to Arweave.
    pub fn flush(&mut self) -> Vec<u8> {
        let mut bundle = Vec::new();
        for item in self.pending_items.drain(..) {
            bundle.extend_from_slice(&item.data);
        }
        bundle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, data: &[u8], added_at: u64) -> BundleItem {
        BundleItem {
            id: id.to_string(),
            data: data.to_vec(),
            added_at,
        }
    }

    #[test]
    fn test_none_strategy_always_flushes() {
        let mut b = DataBundler::new(BundlingStrategy::None);
        b.add_item(item("1", b"hello", 0));
        assert!(b.should_flush(0));
    }

    #[test]
    fn test_empty_never_flushes() {
        let b = DataBundler::new(BundlingStrategy::None);
        assert!(!b.should_flush(0));
    }

    #[test]
    fn test_size_threshold() {
        let mut b = DataBundler::new(BundlingStrategy::SizeThreshold { bytes: 10 });
        b.add_item(item("1", b"12345", 0));
        assert!(!b.should_flush(0)); // 5 < 10
        b.add_item(item("2", b"67890", 0));
        assert!(b.should_flush(0)); // 10 >= 10
    }

    #[test]
    fn test_time_window() {
        let mut b = DataBundler::new(BundlingStrategy::TimeWindow { secs: 60 });
        b.add_item(item("1", b"data", 100));
        assert!(!b.should_flush(150)); // 50 < 60
        assert!(b.should_flush(160)); // 60 >= 60
    }

    #[test]
    fn test_smart_strategy_size() {
        let mut b = DataBundler::new(BundlingStrategy::Smart);
        let big = vec![0u8; 1_048_576]; // 1 MiB
        b.add_item(BundleItem {
            id: "big".to_string(),
            data: big,
            added_at: 0,
        });
        assert!(b.should_flush(0)); // >= 1 MiB
    }

    #[test]
    fn test_smart_strategy_age() {
        let mut b = DataBundler::new(BundlingStrategy::Smart);
        b.add_item(item("1", b"small", 0));
        assert!(!b.should_flush(100)); // 100 < 300
        assert!(b.should_flush(300)); // 300 >= 300
    }

    #[test]
    fn test_smart_strategy_count() {
        let mut b = DataBundler::new(BundlingStrategy::Smart);
        for i in 0..10 {
            b.add_item(item(&format!("{}", i), b"x", 0));
        }
        assert!(b.should_flush(0)); // 10 items
    }

    #[test]
    fn test_flush_concatenates_and_drains() {
        let mut b = DataBundler::new(BundlingStrategy::None);
        b.add_item(item("1", b"hello", 0));
        b.add_item(item("2", b"world", 0));
        assert_eq!(b.pending_count(), 2);
        let data = b.flush();
        assert_eq!(data, b"helloworld");
        assert_eq!(b.pending_count(), 0);
    }
}
