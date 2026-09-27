use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

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

/// Metadata for a tracked pin.
#[derive(Debug, Clone)]
pub struct PinMetadata {
    pub cid: String,
    pub size_bytes: u64,
    pub pinned_at: Instant,
    pub last_accessed: Instant,
}

/// Manages pinning decisions for IPFS content.
#[derive(Debug)]
pub struct PinManager {
    strategy: PinningStrategy,
    total_pinned_bytes: u64,
    pins: HashMap<String, PinMetadata>,
}

impl PinManager {
    /// Create a new pin manager with the given strategy.
    pub fn new(strategy: PinningStrategy) -> Self {
        Self {
            strategy,
            total_pinned_bytes: 0,
            pins: HashMap::new(),
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

    /// Record a pin with full metadata tracking.
    pub fn record_pin_tracked(&mut self, cid: String, size: u64) {
        let now = Instant::now();
        self.total_pinned_bytes += size;
        self.pins.insert(
            cid.clone(),
            PinMetadata {
                cid,
                size_bytes: size,
                pinned_at: now,
                last_accessed: now,
            },
        );
    }

    /// Mark a pin as recently accessed.
    pub fn mark_accessed(&mut self, cid: &str) {
        if let Some(meta) = self.pins.get_mut(cid) {
            meta.last_accessed = Instant::now();
        }
    }

    /// Unpin a tracked entry by CID.
    pub fn record_unpin_tracked(&mut self, cid: &str) -> Option<PinMetadata> {
        if let Some(meta) = self.pins.remove(cid) {
            self.total_pinned_bytes = self.total_pinned_bytes.saturating_sub(meta.size_bytes);
            Some(meta)
        } else {
            None
        }
    }

    /// Return CIDs eligible for rotation (not accessed within `max_idle_secs`).
    pub fn pins_eligible_for_rotation(&self, max_idle_secs: u64) -> Vec<String> {
        let now = Instant::now();
        self.pins
            .values()
            .filter(|meta| now.duration_since(meta.last_accessed).as_secs() >= max_idle_secs)
            .map(|meta| meta.cid.clone())
            .collect()
    }

    /// Rotate (unpin) all pins not accessed within `max_idle_secs`.
    /// Returns the CIDs that were unpinned.
    pub fn rotate_idle_pins(&mut self, max_idle_secs: u64) -> Vec<String> {
        let eligible = self.pins_eligible_for_rotation(max_idle_secs);
        for cid in &eligible {
            self.record_unpin_tracked(cid);
        }
        eligible
    }

    /// Return count of tracked pins.
    pub fn tracked_pin_count(&self) -> usize {
        self.pins.len()
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
        assert!(pm.should_pin(200, 0)); // 800 + 200 = 1000
    }

    #[test]
    fn test_smart_strategy() {
        let pm = PinManager::new(PinningStrategy::Smart);
        // Needs > 2 accesses AND < 100 MiB
        assert!(!pm.should_pin(100, 0)); // too few accesses
        assert!(!pm.should_pin(100, 2)); // exactly 2, need > 2
        assert!(pm.should_pin(100, 3)); // good
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

    #[test]
    fn test_record_pin_tracked() {
        let mut pm = PinManager::new(PinningStrategy::PinAll);
        pm.record_pin_tracked("Qm123".to_string(), 1000);
        assert_eq!(pm.tracked_pin_count(), 1);
        assert_eq!(pm.total_pinned_bytes(), 1000);
    }

    #[test]
    fn test_unpin_tracked() {
        let mut pm = PinManager::new(PinningStrategy::PinAll);
        pm.record_pin_tracked("Qm123".to_string(), 1000);
        let meta = pm.record_unpin_tracked("Qm123");
        assert!(meta.is_some());
        assert_eq!(pm.tracked_pin_count(), 0);
        assert_eq!(pm.total_pinned_bytes(), 0);
    }

    #[test]
    fn test_mark_accessed_updates_last_accessed() {
        let mut pm = PinManager::new(PinningStrategy::PinAll);
        pm.record_pin_tracked("Qm123".to_string(), 100);
        let initial = pm.pins.get("Qm123").unwrap().last_accessed;
        // Small sleep to ensure time difference
        std::thread::sleep(std::time::Duration::from_millis(10));
        pm.mark_accessed("Qm123");
        let updated = pm.pins.get("Qm123").unwrap().last_accessed;
        assert!(updated > initial);
    }

    #[test]
    fn test_rotation_returns_idle_pins() {
        let mut pm = PinManager::new(PinningStrategy::PinAll);
        pm.record_pin_tracked("Qm-old".to_string(), 100);
        // Manually backdate the pin for testing
        if let Some(meta) = pm.pins.get_mut("Qm-old") {
            meta.last_accessed = Instant::now() - std::time::Duration::from_secs(3600);
        }
        pm.record_pin_tracked("Qm-new".to_string(), 200);

        let eligible = pm.pins_eligible_for_rotation(1800); // 30 min idle threshold
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0], "Qm-old");
    }

    #[test]
    fn test_rotate_idle_pins_removes_them() {
        let mut pm = PinManager::new(PinningStrategy::PinAll);
        pm.record_pin_tracked("Qm-old".to_string(), 500);
        if let Some(meta) = pm.pins.get_mut("Qm-old") {
            meta.last_accessed = Instant::now() - std::time::Duration::from_secs(7200);
        }
        pm.record_pin_tracked("Qm-fresh".to_string(), 300);

        let rotated = pm.rotate_idle_pins(3600);
        assert_eq!(rotated.len(), 1);
        assert_eq!(rotated[0], "Qm-old");
        assert_eq!(pm.tracked_pin_count(), 1);
        assert_eq!(pm.total_pinned_bytes(), 300);
    }
}
