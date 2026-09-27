//! Content-addressed deduplication registry across storage providers.
//!
//! Tracks canonical blake3 hashes and which providers already hold copies of each blob,
//! enabling skip-if-exists logic to avoid redundant storage operations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::ContentId;

/// A single deduplication entry tracking all known copies of a blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupEntry {
    /// Canonical blake3 hash (64-char hex string).
    pub canonical_hash: String,
    /// Size of the blob in bytes.
    pub size_bytes: u64,
    /// Provider-specific ContentIds that already hold this blob.
    pub provider_copies: Vec<ContentId>,
    /// Reference count (how many logical objects reference this blob).
    pub ref_count: u64,
    /// Unix timestamp when this blob was first stored.
    pub first_stored_at: u64,
}

impl DedupEntry {
    /// Create a new deduplication entry.
    pub fn new(
        canonical_hash: String,
        size_bytes: u64,
        first_content_id: ContentId,
        first_stored_at: u64,
    ) -> Self {
        Self {
            canonical_hash,
            size_bytes,
            provider_copies: vec![first_content_id],
            ref_count: 1,
            first_stored_at,
        }
    }

    /// Check if a provider already has a copy of this blob.
    pub fn has_provider(&self, provider_name: &str) -> bool {
        self.provider_copies
            .iter()
            .any(|cid| cid.provider == provider_name)
    }

    /// Get the ContentId for a specific provider, if available.
    pub fn get_content_id(&self, provider_name: &str) -> Option<&ContentId> {
        self.provider_copies
            .iter()
            .find(|cid| cid.provider == provider_name)
    }

    /// Add a new provider copy and return true if this is a new provider.
    pub fn add_provider_copy(&mut self, content_id: ContentId) -> bool {
        if self.has_provider(&content_id.provider) {
            return false;
        }
        self.provider_copies.push(content_id);
        true
    }

    /// Increment the reference count.
    pub fn increment_ref_count(&mut self) {
        self.ref_count = self.ref_count.saturating_add(1);
    }

    /// Decrement the reference count and return true if it reaches zero.
    pub fn decrement_ref_count(&mut self) -> bool {
        if self.ref_count > 0 {
            self.ref_count -= 1;
        }
        self.ref_count == 0
    }
}

/// Deduplication registry managing content across multiple storage providers.
///
/// Thread-safe registry that tracks canonical hashes and provider-specific copies.
/// Enables skip-if-exists logic to avoid redundant stores and supports reference counting
/// for safe cleanup.
pub struct DeduplicationRegistry {
    entries: RwLock<HashMap<String, DedupEntry>>,
}

impl DeduplicationRegistry {
    /// Create a new empty deduplication registry.
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Insert a new blob into the registry or add a provider copy to an existing entry.
    ///
    /// Returns the DedupEntry after insertion/update. Does not increment ref_count;
    /// that should be done explicitly via increment_ref() for logical references.
    pub fn insert(
        &self,
        canonical_hash: String,
        size_bytes: u64,
        content_id: ContentId,
        timestamp: u64,
    ) -> Result<DedupEntry, String> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| format!("failed to acquire write lock: {e}"))?;

        let entry = entries.entry(canonical_hash.clone()).or_insert_with(|| {
            DedupEntry::new(canonical_hash, size_bytes, content_id.clone(), timestamp)
        });

        entry.add_provider_copy(content_id);

        Ok(entry.clone())
    }

    /// Look up a canonical hash and check if a specific provider has it.
    ///
    /// Returns the ContentId if found, None otherwise.
    pub fn lookup(
        &self,
        canonical_hash: &str,
        provider_name: &str,
    ) -> Result<Option<ContentId>, String> {
        let entries = self
            .entries
            .read()
            .map_err(|e| format!("failed to acquire read lock: {e}"))?;

        Ok(entries
            .get(canonical_hash)
            .and_then(|entry| entry.get_content_id(provider_name).cloned()))
    }

    /// Check if a canonical hash is stored on any provider.
    pub fn exists(&self, canonical_hash: &str) -> Result<bool, String> {
        let entries = self
            .entries
            .read()
            .map_err(|e| format!("failed to acquire read lock: {e}"))?;

        Ok(entries.contains_key(canonical_hash))
    }

    /// Get the reference count for a canonical hash.
    pub fn get_ref_count(&self, canonical_hash: &str) -> Result<u64, String> {
        let entries = self
            .entries
            .read()
            .map_err(|e| format!("failed to acquire read lock: {e}"))?;

        Ok(entries
            .get(canonical_hash)
            .map(|entry| entry.ref_count)
            .unwrap_or(0))
    }

    /// Increment the reference count for a canonical hash.
    pub fn increment_ref(&self, canonical_hash: &str) -> Result<u64, String> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| format!("failed to acquire write lock: {e}"))?;

        if let Some(entry) = entries.get_mut(canonical_hash) {
            entry.increment_ref_count();
            Ok(entry.ref_count)
        } else {
            Err(format!("canonical hash not found: {canonical_hash}"))
        }
    }

    /// Decrement the reference count and return true if it reaches zero.
    pub fn decrement_ref(&self, canonical_hash: &str) -> Result<bool, String> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| format!("failed to acquire write lock: {e}"))?;

        if let Some(entry) = entries.get_mut(canonical_hash) {
            Ok(entry.decrement_ref_count())
        } else {
            Err(format!("canonical hash not found: {canonical_hash}"))
        }
    }

    /// Remove a canonical hash entry from the registry.
    ///
    /// Returns the removed entry if it existed.
    pub fn remove(&self, canonical_hash: &str) -> Result<Option<DedupEntry>, String> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| format!("failed to acquire write lock: {e}"))?;

        Ok(entries.remove(canonical_hash))
    }

    /// Get a snapshot of the current registry state for persistence.
    pub fn snapshot(&self) -> Result<Vec<(String, DedupEntry)>, String> {
        let entries = self
            .entries
            .read()
            .map_err(|e| format!("failed to acquire read lock: {e}"))?;

        Ok(entries
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    /// Restore the registry from a snapshot.
    pub fn restore(&self, snapshot: Vec<(String, DedupEntry)>) -> Result<(), String> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| format!("failed to acquire write lock: {e}"))?;

        *entries = snapshot.into_iter().collect();
        Ok(())
    }

    /// Clear all entries from the registry.
    pub fn clear(&self) -> Result<(), String> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| format!("failed to acquire write lock: {e}"))?;

        entries.clear();
        Ok(())
    }

    /// Get the number of unique canonical hashes in the registry.
    pub fn len(&self) -> Result<usize, String> {
        let entries = self
            .entries
            .read()
            .map_err(|e| format!("failed to acquire read lock: {e}"))?;

        Ok(entries.len())
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> Result<bool, String> {
        let entries = self
            .entries
            .read()
            .map_err(|e| format!("failed to acquire read lock: {e}"))?;

        Ok(entries.is_empty())
    }
}

impl Default for DeduplicationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_dedup_entry() {
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let entry = DedupEntry::new("abc123".to_string(), 1024, cid.clone(), 1000);

        assert_eq!(entry.canonical_hash, "abc123");
        assert_eq!(entry.size_bytes, 1024);
        assert_eq!(entry.provider_copies.len(), 1);
        assert_eq!(entry.ref_count, 1);
        assert_eq!(entry.first_stored_at, 1000);
    }

    #[test]
    fn test_dedup_entry_has_provider() {
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let entry = DedupEntry::new("abc123".to_string(), 1024, cid, 1000);

        assert!(entry.has_provider("ipfs"));
        assert!(!entry.has_provider("filecoin"));
    }

    #[test]
    fn test_dedup_entry_add_provider_copy() {
        let cid1 = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let mut entry = DedupEntry::new("abc123".to_string(), 1024, cid1, 1000);

        let cid2 = ContentId {
            provider: "filecoin".to_string(),
            hash: "bafy456".to_string(),
        };
        let is_new = entry.add_provider_copy(cid2);
        assert!(is_new);
        assert_eq!(entry.provider_copies.len(), 2);

        // Adding same provider again should return false
        let cid3 = ContentId {
            provider: "filecoin".to_string(),
            hash: "bafy789".to_string(),
        };
        let is_new = entry.add_provider_copy(cid3);
        assert!(!is_new);
        assert_eq!(entry.provider_copies.len(), 2);
    }

    #[test]
    fn test_dedup_entry_ref_count() {
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let mut entry = DedupEntry::new("abc123".to_string(), 1024, cid, 1000);

        assert_eq!(entry.ref_count, 1);

        entry.increment_ref_count();
        assert_eq!(entry.ref_count, 2);

        entry.increment_ref_count();
        assert_eq!(entry.ref_count, 3);

        let at_zero = entry.decrement_ref_count();
        assert!(!at_zero);
        assert_eq!(entry.ref_count, 2);

        entry.decrement_ref_count();
        let at_zero = entry.decrement_ref_count();
        assert!(at_zero);
        assert_eq!(entry.ref_count, 0);
    }

    #[test]
    fn test_registry_insert_and_lookup() {
        let registry = DeduplicationRegistry::new();
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };

        let result = registry.insert("abc123".to_string(), 1024, cid.clone(), 1000);
        assert!(result.is_ok());

        let lookup = registry.lookup("abc123", "ipfs");
        assert!(lookup.is_ok());
        assert_eq!(lookup.unwrap(), Some(cid));
    }

    #[test]
    fn test_registry_lookup_nonexistent_hash() {
        let registry = DeduplicationRegistry::new();
        let lookup = registry.lookup("nonexistent", "ipfs");
        assert!(lookup.is_ok());
        assert_eq!(lookup.unwrap(), None);
    }

    #[test]
    fn test_registry_lookup_wrong_provider() {
        let registry = DeduplicationRegistry::new();
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };

        let _ = registry.insert("abc123".to_string(), 1024, cid, 1000);

        let lookup = registry.lookup("abc123", "filecoin");
        assert!(lookup.is_ok());
        assert_eq!(lookup.unwrap(), None);
    }

    #[test]
    fn test_registry_exists() {
        let registry = DeduplicationRegistry::new();
        assert!(!registry.exists("nonexistent").unwrap());

        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let _ = registry.insert("abc123".to_string(), 1024, cid, 1000);

        assert!(registry.exists("abc123").unwrap());
    }

    #[test]
    fn test_registry_ref_count() {
        let registry = DeduplicationRegistry::new();
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };

        let _ = registry.insert("abc123".to_string(), 1024, cid, 1000);
        assert_eq!(registry.get_ref_count("abc123").unwrap(), 1);

        let _ = registry.increment_ref("abc123");
        assert_eq!(registry.get_ref_count("abc123").unwrap(), 2);

        let at_zero = registry.decrement_ref("abc123");
        assert!(!at_zero.unwrap());
        assert_eq!(registry.get_ref_count("abc123").unwrap(), 1);
    }

    #[test]
    fn test_registry_decrement_to_zero() {
        let registry = DeduplicationRegistry::new();
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };

        let _ = registry.insert("abc123".to_string(), 1024, cid, 1000);

        let at_zero = registry.decrement_ref("abc123");
        assert!(at_zero.unwrap());
    }

    #[test]
    fn test_registry_multiple_providers() {
        let registry = DeduplicationRegistry::new();

        let cid1 = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let _ = registry.insert("abc123".to_string(), 1024, cid1.clone(), 1000);

        let cid2 = ContentId {
            provider: "filecoin".to_string(),
            hash: "bafy456".to_string(),
        };
        let _ = registry.insert("abc123".to_string(), 1024, cid2.clone(), 1000);

        assert_eq!(registry.lookup("abc123", "ipfs").unwrap(), Some(cid1));
        assert_eq!(registry.lookup("abc123", "filecoin").unwrap(), Some(cid2));
    }

    #[test]
    fn test_registry_snapshot_and_restore() {
        let registry = DeduplicationRegistry::new();

        let cid1 = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let _ = registry.insert("abc123".to_string(), 1024, cid1.clone(), 1000);

        let cid2 = ContentId {
            provider: "filecoin".to_string(),
            hash: "bafy456".to_string(),
        };
        let _ = registry.insert("def456".to_string(), 2048, cid2.clone(), 2000);

        let snapshot = registry.snapshot().unwrap();
        assert_eq!(snapshot.len(), 2);

        let new_registry = DeduplicationRegistry::new();
        new_registry.restore(snapshot).unwrap();

        assert_eq!(new_registry.lookup("abc123", "ipfs").unwrap(), Some(cid1));
        assert_eq!(
            new_registry.lookup("def456", "filecoin").unwrap(),
            Some(cid2)
        );
    }

    #[test]
    fn test_registry_clear() {
        let registry = DeduplicationRegistry::new();

        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let _ = registry.insert("abc123".to_string(), 1024, cid, 1000);

        assert_eq!(registry.len().unwrap(), 1);
        registry.clear().unwrap();
        assert_eq!(registry.len().unwrap(), 0);
    }

    #[test]
    fn test_registry_remove() {
        let registry = DeduplicationRegistry::new();

        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let _ = registry.insert("abc123".to_string(), 1024, cid, 1000);

        let removed = registry.remove("abc123").unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().canonical_hash, "abc123");
        assert_eq!(registry.len().unwrap(), 0);
    }

    #[test]
    fn test_registry_is_empty() {
        let registry = DeduplicationRegistry::new();
        assert!(registry.is_empty().unwrap());

        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "Qm123".to_string(),
        };
        let _ = registry.insert("abc123".to_string(), 1024, cid, 1000);
        assert!(!registry.is_empty().unwrap());
    }
}
