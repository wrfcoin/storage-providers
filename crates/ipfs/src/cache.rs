use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use lru::LruCache;

struct CacheEntry {
    data: Vec<u8>,
    inserted_at: Instant,
}

/// A bounded LRU cache with per-entry TTL expiration.
pub struct TtlLruCache {
    inner: LruCache<String, CacheEntry>,
    ttl: Duration,
}

impl TtlLruCache {
    /// Create a new cache with the given capacity and TTL.
    pub fn new(capacity: usize, ttl_secs: u64) -> Self {
        Self {
            inner: LruCache::new(NonZeroUsize::new(capacity.max(1)).unwrap()),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Get data by key. Returns `None` if missing or TTL expired.
    pub fn get(&mut self, key: &str) -> Option<Vec<u8>> {
        // Peek first to check TTL without promoting.
        let expired = self
            .inner
            .peek(key)
            .is_some_and(|e| e.inserted_at.elapsed() > self.ttl);

        if expired {
            self.inner.pop(key);
            return None;
        }

        self.inner.get(key).map(|e| e.data.clone())
    }

    /// Insert data into the cache.
    pub fn insert(&mut self, key: String, data: Vec<u8>) {
        self.inner.put(
            key,
            CacheEntry {
                data,
                inserted_at: Instant::now(),
            },
        );
    }

    /// Remove an entry from the cache. Returns true if it existed.
    pub fn remove(&mut self, key: &str) -> bool {
        self.inner.pop(key).is_some()
    }

    /// Check if a key exists and is not expired.
    pub fn contains(&mut self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Number of entries currently in the cache (may include expired entries).
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let mut cache = TtlLruCache::new(10, 3600);
        cache.insert("k1".to_string(), b"hello".to_vec());
        assert_eq!(cache.get("k1"), Some(b"hello".to_vec()));
        assert_eq!(cache.get("missing"), None);
    }

    #[test]
    fn test_eviction_at_capacity() {
        let mut cache = TtlLruCache::new(2, 3600);
        cache.insert("a".to_string(), b"1".to_vec());
        cache.insert("b".to_string(), b"2".to_vec());
        cache.insert("c".to_string(), b"3".to_vec()); // evicts "a"
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("b"), Some(b"2".to_vec()));
        assert_eq!(cache.get("c"), Some(b"3".to_vec()));
    }

    #[test]
    fn test_ttl_expiry() {
        // Use 0-second TTL to guarantee expiry.
        let mut cache = TtlLruCache::new(10, 0);
        cache.insert("k".to_string(), b"data".to_vec());
        // With TTL=0, any elapsed time causes expiry.
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn test_remove() {
        let mut cache = TtlLruCache::new(10, 3600);
        cache.insert("k".to_string(), b"data".to_vec());
        assert!(cache.remove("k"));
        assert!(!cache.remove("k"));
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn test_len() {
        let mut cache = TtlLruCache::new(10, 3600);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
        cache.insert("a".to_string(), b"1".to_vec());
        cache.insert("b".to_string(), b"2".to_vec());
        assert_eq!(cache.len(), 2);
    }
}
