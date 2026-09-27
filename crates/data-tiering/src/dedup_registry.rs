//! Persistent content-addressed deduplication registry across storage providers.
//!
//! Maintains a registry of content-hash → providers mapping, enabling the same content
//! (by CID) stored on multiple providers to be tracked as ONE logical item with multiple
//! physical replicas. Backed by sled for persistence.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// Provider metadata for a content replica.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderReplica {
    /// Provider identifier (e.g., "ipfs", "filecoin", "arweave").
    pub provider_id: String,
    /// Storage location identifier (CID, transaction ID, deal ID, etc.).
    pub storage_location: String,
    /// Unix timestamp when content was pinned/stored.
    pub pinned_at: u64,
    /// Unix timestamp of last verification.
    pub last_verified: u64,
}

impl ProviderReplica {
    /// Create a new provider replica entry.
    pub fn new(provider_id: String, storage_location: String, pinned_at: u64) -> Self {
        Self {
            provider_id,
            storage_location,
            pinned_at,
            last_verified: pinned_at,
        }
    }
}

/// A registry entry for a single piece of content and all its replicas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// Content hash (blake3 or CID in canonical form).
    pub content_hash: String,
    /// List of providers holding this content.
    pub providers: Vec<ProviderReplica>,
    /// Reference count (how many logical objects reference this content).
    pub ref_count: u64,
    /// Unix timestamp when content was first seen.
    pub first_seen: u64,
    /// Total size of the content in bytes.
    pub total_size_bytes: u64,
}

impl RegistryEntry {
    /// Create a new registry entry.
    pub fn new(
        content_hash: String,
        size_bytes: u64,
        provider_id: String,
        storage_location: String,
        timestamp: u64,
    ) -> Self {
        let replica = ProviderReplica::new(provider_id, storage_location, timestamp);
        Self {
            content_hash,
            providers: vec![replica],
            ref_count: 1,
            first_seen: timestamp,
            total_size_bytes: size_bytes,
        }
    }

    /// Check if a provider already holds this content.
    pub fn has_provider(&self, provider_id: &str) -> bool {
        self.providers.iter().any(|p| p.provider_id == provider_id)
    }

    /// Get the replica for a specific provider.
    pub fn get_provider(&self, provider_id: &str) -> Option<&ProviderReplica> {
        self.providers.iter().find(|p| p.provider_id == provider_id)
    }

    /// Get mutable replica for a specific provider.
    pub fn get_provider_mut(&mut self, provider_id: &str) -> Option<&mut ProviderReplica> {
        self.providers
            .iter_mut()
            .find(|p| p.provider_id == provider_id)
    }

    /// Add a new provider replica (returns false if provider already exists).
    pub fn add_provider(
        &mut self,
        provider_id: String,
        storage_location: String,
        pinned_at: u64,
    ) -> bool {
        if self.has_provider(&provider_id) {
            return false;
        }
        self.providers.push(ProviderReplica::new(
            provider_id,
            storage_location,
            pinned_at,
        ));
        true
    }

    /// Remove a provider replica (returns true if removed).
    pub fn remove_provider(&mut self, provider_id: &str) -> bool {
        let initial_len = self.providers.len();
        self.providers.retain(|p| p.provider_id != provider_id);
        self.providers.len() < initial_len
    }

    /// Update last verification timestamp for a provider.
    pub fn update_verification(&mut self, provider_id: &str, timestamp: u64) -> bool {
        if let Some(replica) = self.get_provider_mut(provider_id) {
            replica.last_verified = timestamp;
            true
        } else {
            false
        }
    }
}

/// Errors that can occur in the dedup registry.
#[derive(Debug, Error)]
pub enum DedupError {
    #[error("sled error: {0}")]
    SledError(String),
    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
}

/// Persistent deduplication registry backed by sled.
pub struct PersistentDedupRegistry {
    db: sled::Db,
    /// In-memory cache of registry entries for performance.
    cache: std::sync::RwLock<HashMap<String, RegistryEntry>>,
}

/// Metrics for the dedup registry.
#[derive(Debug, Clone, Default)]
pub struct DedupMetrics {
    /// Total number of registry entries.
    pub registry_entries_total: u64,
    /// Total number of provider replicas.
    pub registry_providers_total: u64,
    /// Number of deduplication opportunities (CIDs on ≥2 providers).
    pub dedup_opportunities: u64,
    /// Number of orphaned CIDs (ref_count == 0).
    pub orphan_count: u64,
}

impl PersistentDedupRegistry {
    /// Create or open an existing persistent registry at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, DedupError> {
        let db = sled::open(path).map_err(|e| DedupError::SledError(e.to_string()))?;
        let registry = Self {
            db,
            cache: std::sync::RwLock::new(HashMap::new()),
        };
        registry.load_cache()?;
        Ok(registry)
    }

    /// Load all entries from sled into the in-memory cache.
    fn load_cache(&self) -> Result<(), DedupError> {
        let mut cache = self
            .cache
            .write()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;
        cache.clear();

        for item in self.db.iter() {
            let (key, value) = item.map_err(|e| DedupError::SledError(e.to_string()))?;
            let key_str = String::from_utf8(key.to_vec())
                .map_err(|e| DedupError::InvalidState(e.to_string()))?;
            let entry: RegistryEntry = serde_json::from_slice(&value)?;
            cache.insert(key_str, entry);
        }

        Ok(())
    }

    /// Register content with a provider.
    pub fn register(
        &self,
        content_hash: String,
        provider_id: String,
        storage_location: String,
        size_bytes: u64,
        timestamp: u64,
    ) -> Result<RegistryEntry, DedupError> {
        let mut cache = self
            .cache
            .write()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;

        let entry = cache.entry(content_hash.clone()).or_insert_with(|| {
            RegistryEntry::new(
                content_hash.clone(),
                size_bytes,
                provider_id.clone(),
                storage_location.clone(),
                timestamp,
            )
        });

        let _is_new_provider = entry.add_provider(provider_id, storage_location, timestamp);

        // Persist to sled
        let serialized = serde_json::to_vec(entry)?;
        self.db
            .insert(&content_hash, serialized)
            .map_err(|e| DedupError::SledError(e.to_string()))?;

        Ok(entry.clone())
    }

    /// Unregister a provider from a content entry.
    pub fn unregister(&self, content_hash: &str, provider_id: &str) -> Result<bool, DedupError> {
        let mut cache = self
            .cache
            .write()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;

        if let Some(entry) = cache.get_mut(content_hash) {
            let removed = entry.remove_provider(provider_id);
            if removed {
                // Update sled
                let serialized = serde_json::to_vec(entry)?;
                self.db
                    .insert(content_hash, serialized)
                    .map_err(|e| DedupError::SledError(e.to_string()))?;
            }
            Ok(removed)
        } else {
            Err(DedupError::NotFound(content_hash.to_string()))
        }
    }

    /// Increment reference count for a content hash.
    pub fn incref(&self, content_hash: &str) -> Result<u64, DedupError> {
        let mut cache = self
            .cache
            .write()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;

        if let Some(entry) = cache.get_mut(content_hash) {
            entry.ref_count = entry.ref_count.saturating_add(1);
            let serialized = serde_json::to_vec(entry)?;
            self.db
                .insert(content_hash, serialized)
                .map_err(|e| DedupError::SledError(e.to_string()))?;
            Ok(entry.ref_count)
        } else {
            Err(DedupError::NotFound(content_hash.to_string()))
        }
    }

    /// Decrement reference count for a content hash.
    pub fn decref(&self, content_hash: &str) -> Result<u64, DedupError> {
        let mut cache = self
            .cache
            .write()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;

        if let Some(entry) = cache.get_mut(content_hash) {
            if entry.ref_count > 0 {
                entry.ref_count -= 1;
            }
            let serialized = serde_json::to_vec(entry)?;
            self.db
                .insert(content_hash, serialized)
                .map_err(|e| DedupError::SledError(e.to_string()))?;
            Ok(entry.ref_count)
        } else {
            Err(DedupError::NotFound(content_hash.to_string()))
        }
    }

    /// Look up a content entry by hash.
    pub fn lookup(&self, content_hash: &str) -> Result<Option<RegistryEntry>, DedupError> {
        let cache = self
            .cache
            .read()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;
        Ok(cache.get(content_hash).cloned())
    }

    /// Find all content stored on multiple providers (deduplication opportunities).
    pub fn find_duplicates(&self) -> Result<Vec<RegistryEntry>, DedupError> {
        let cache = self
            .cache
            .read()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;

        let duplicates = cache
            .values()
            .filter(|entry| entry.providers.len() >= 2)
            .cloned()
            .collect();

        Ok(duplicates)
    }

    /// Find all orphaned content (ref_count == 0).
    pub fn orphans(&self) -> Result<Vec<String>, DedupError> {
        let cache = self
            .cache
            .read()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;

        let orphaned = cache
            .iter()
            .filter(|(_, entry)| entry.ref_count == 0)
            .map(|(hash, _)| hash.clone())
            .collect();

        Ok(orphaned)
    }

    /// Update verification timestamp for a provider replica.
    pub fn update_verification(
        &self,
        content_hash: &str,
        provider_id: &str,
        timestamp: u64,
    ) -> Result<bool, DedupError> {
        let mut cache = self
            .cache
            .write()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;

        if let Some(entry) = cache.get_mut(content_hash) {
            let updated = entry.update_verification(provider_id, timestamp);
            if updated {
                let serialized = serde_json::to_vec(entry)?;
                self.db
                    .insert(content_hash, serialized)
                    .map_err(|e| DedupError::SledError(e.to_string()))?;
            }
            Ok(updated)
        } else {
            Err(DedupError::NotFound(content_hash.to_string()))
        }
    }

    /// Get current metrics.
    pub fn metrics(&self) -> Result<DedupMetrics, DedupError> {
        let cache = self
            .cache
            .read()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;

        let registry_entries_total = cache.len() as u64;
        let registry_providers_total = cache.values().map(|e| e.providers.len() as u64).sum();
        let dedup_opportunities = cache.values().filter(|e| e.providers.len() >= 2).count() as u64;
        let orphan_count = cache.values().filter(|e| e.ref_count == 0).count() as u64;

        Ok(DedupMetrics {
            registry_entries_total,
            registry_providers_total,
            dedup_opportunities,
            orphan_count,
        })
    }

    /// Get the number of entries in the registry.
    pub fn len(&self) -> Result<usize, DedupError> {
        let cache = self
            .cache
            .read()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;
        Ok(cache.len())
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> Result<bool, DedupError> {
        Ok(self.len()? == 0)
    }

    /// Get all entries (for testing/debugging).
    pub fn all_entries(&self) -> Result<Vec<RegistryEntry>, DedupError> {
        let cache = self
            .cache
            .read()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;
        Ok(cache.values().cloned().collect())
    }

    /// Clear all entries from the registry.
    pub fn clear(&self) -> Result<(), DedupError> {
        self.db
            .clear()
            .map_err(|e| DedupError::SledError(e.to_string()))?;
        let mut cache = self
            .cache
            .write()
            .map_err(|e| DedupError::InvalidState(e.to_string()))?;
        cache.clear();
        Ok(())
    }
}

/// Adapter module for integration with GC (sp#59).
/// This provides the interface that the GC orphan detection should use.
/// TODO: Update GC to query this registry instead of its local check.
pub mod gc_adapter {
    use super::*;

    /// Integration point for garbage collection orphan detection.
    /// The GC should call this method to find candidates for deletion.
    pub fn get_orphaned_hashes(
        registry: &PersistentDedupRegistry,
    ) -> Result<Vec<String>, DedupError> {
        registry.orphans()
    }

    /// Integration point for finding dedup opportunities.
    /// The GC can use this to prioritize consolidation/migration.
    pub fn get_duplication_candidates(
        registry: &PersistentDedupRegistry,
    ) -> Result<Vec<RegistryEntry>, DedupError> {
        registry.find_duplicates()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_registry() -> (PersistentDedupRegistry, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let registry = PersistentDedupRegistry::open(temp_dir.path()).unwrap();
        (registry, temp_dir)
    }

    #[test]
    fn test_new_registry_entry() {
        let entry = RegistryEntry::new(
            "hash123".to_string(),
            1024,
            "ipfs".to_string(),
            "Qm123".to_string(),
            1000,
        );

        assert_eq!(entry.content_hash, "hash123");
        assert_eq!(entry.total_size_bytes, 1024);
        assert_eq!(entry.providers.len(), 1);
        assert_eq!(entry.ref_count, 1);
        assert_eq!(entry.first_seen, 1000);
    }

    #[test]
    fn test_register_new_content() {
        let (registry, _temp) = setup_registry();

        let result = registry.register(
            "hash123".to_string(),
            "ipfs".to_string(),
            "Qm123".to_string(),
            1024,
            1000,
        );

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.content_hash, "hash123");
        assert_eq!(entry.providers.len(), 1);
        assert_eq!(entry.providers[0].provider_id, "ipfs");
    }

    #[test]
    fn test_register_same_content_second_provider() {
        let (registry, _temp) = setup_registry();

        registry
            .register(
                "hash123".to_string(),
                "ipfs".to_string(),
                "Qm123".to_string(),
                1024,
                1000,
            )
            .unwrap();

        let result = registry.register(
            "hash123".to_string(),
            "filecoin".to_string(),
            "bafy456".to_string(),
            1024,
            1100,
        );

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.providers.len(), 2);
        assert!(entry.has_provider("ipfs"));
        assert!(entry.has_provider("filecoin"));
    }

    #[test]
    fn test_unregister_provider() {
        let (registry, _temp) = setup_registry();

        registry
            .register(
                "hash123".to_string(),
                "ipfs".to_string(),
                "Qm123".to_string(),
                1024,
                1000,
            )
            .unwrap();

        registry
            .register(
                "hash123".to_string(),
                "filecoin".to_string(),
                "bafy456".to_string(),
                1024,
                1100,
            )
            .unwrap();

        let result = registry.unregister("hash123", "filecoin");
        assert!(result.is_ok());
        assert!(result.unwrap());

        let entry = registry.lookup("hash123").unwrap().unwrap();
        assert_eq!(entry.providers.len(), 1);
        assert!(entry.has_provider("ipfs"));
        assert!(!entry.has_provider("filecoin"));
    }

    #[test]
    fn test_unregister_nonexistent_hash() {
        let (registry, _temp) = setup_registry();

        let result = registry.unregister("nonexistent", "ipfs");
        assert!(result.is_err());
    }

    #[test]
    fn test_incref_decref_lifecycle() {
        let (registry, _temp) = setup_registry();

        registry
            .register(
                "hash123".to_string(),
                "ipfs".to_string(),
                "Qm123".to_string(),
                1024,
                1000,
            )
            .unwrap();

        assert_eq!(registry.lookup("hash123").unwrap().unwrap().ref_count, 1);

        registry.incref("hash123").unwrap();
        assert_eq!(registry.lookup("hash123").unwrap().unwrap().ref_count, 2);

        registry.incref("hash123").unwrap();
        assert_eq!(registry.lookup("hash123").unwrap().unwrap().ref_count, 3);

        registry.decref("hash123").unwrap();
        assert_eq!(registry.lookup("hash123").unwrap().unwrap().ref_count, 2);

        registry.decref("hash123").unwrap();
        registry.decref("hash123").unwrap();
        assert_eq!(registry.lookup("hash123").unwrap().unwrap().ref_count, 0);
    }

    #[test]
    fn test_find_duplicates() {
        let (registry, _temp) = setup_registry();

        registry
            .register(
                "hash1".to_string(),
                "ipfs".to_string(),
                "Qm1".to_string(),
                1024,
                1000,
            )
            .unwrap();

        registry
            .register(
                "hash2".to_string(),
                "ipfs".to_string(),
                "Qm2".to_string(),
                2048,
                1000,
            )
            .unwrap();
        registry
            .register(
                "hash2".to_string(),
                "filecoin".to_string(),
                "bafy2".to_string(),
                2048,
                1100,
            )
            .unwrap();

        let duplicates = registry.find_duplicates().unwrap();
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].content_hash, "hash2");
    }

    #[test]
    fn test_orphans() {
        let (registry, _temp) = setup_registry();

        registry
            .register(
                "hash1".to_string(),
                "ipfs".to_string(),
                "Qm1".to_string(),
                1024,
                1000,
            )
            .unwrap();

        registry
            .register(
                "hash2".to_string(),
                "ipfs".to_string(),
                "Qm2".to_string(),
                2048,
                1000,
            )
            .unwrap();

        registry.incref("hash1").unwrap();
        registry.decref("hash1").unwrap();
        registry.decref("hash2").unwrap();

        let orphaned = registry.orphans().unwrap();
        assert_eq!(orphaned.len(), 1);
        assert!(orphaned.contains(&"hash2".to_string()));
    }

    #[test]
    fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();

        {
            let registry = PersistentDedupRegistry::open(path).unwrap();
            registry
                .register(
                    "hash123".to_string(),
                    "ipfs".to_string(),
                    "Qm123".to_string(),
                    1024,
                    1000,
                )
                .unwrap();
            registry
                .register(
                    "hash123".to_string(),
                    "filecoin".to_string(),
                    "bafy456".to_string(),
                    1024,
                    1100,
                )
                .unwrap();
        }

        // Reopen and verify data persisted
        let registry = PersistentDedupRegistry::open(path).unwrap();
        let entry = registry.lookup("hash123").unwrap().unwrap();
        assert_eq!(entry.providers.len(), 2);
        assert!(entry.has_provider("ipfs"));
        assert!(entry.has_provider("filecoin"));
    }

    #[test]
    fn test_concurrent_operations() {
        let (registry, _temp) = setup_registry();
        let registry = std::sync::Arc::new(registry);

        let mut handles = vec![];

        for i in 0..4 {
            let reg = registry.clone();
            let handle = std::thread::spawn(move || {
                for j in 0..5 {
                    let hash = format!("hash_{}", i * 5 + j);
                    let provider = format!("provider_{}", i);
                    let _ = reg.register(hash, provider, "loc".to_string(), 1024, 1000);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(registry.len().unwrap(), 20);
    }

    #[test]
    fn test_update_verification() {
        let (registry, _temp) = setup_registry();

        registry
            .register(
                "hash123".to_string(),
                "ipfs".to_string(),
                "Qm123".to_string(),
                1024,
                1000,
            )
            .unwrap();

        let result = registry.update_verification("hash123", "ipfs", 2000);
        assert!(result.is_ok());
        assert!(result.unwrap());

        let entry = registry.lookup("hash123").unwrap().unwrap();
        let provider = entry.get_provider("ipfs").unwrap();
        assert_eq!(provider.last_verified, 2000);
    }

    #[test]
    fn test_metrics() {
        let (registry, _temp) = setup_registry();

        registry
            .register(
                "hash1".to_string(),
                "ipfs".to_string(),
                "Qm1".to_string(),
                1024,
                1000,
            )
            .unwrap();

        registry
            .register(
                "hash2".to_string(),
                "ipfs".to_string(),
                "Qm2".to_string(),
                2048,
                1000,
            )
            .unwrap();
        registry
            .register(
                "hash2".to_string(),
                "filecoin".to_string(),
                "bafy2".to_string(),
                2048,
                1100,
            )
            .unwrap();

        registry.decref("hash2").unwrap();

        let metrics = registry.metrics().unwrap();
        assert_eq!(metrics.registry_entries_total, 2);
        assert_eq!(metrics.registry_providers_total, 3);
        assert_eq!(metrics.dedup_opportunities, 1);
        assert_eq!(metrics.orphan_count, 1);
    }

    #[test]
    fn test_gc_adapter_orphans() {
        let (registry, _temp) = setup_registry();

        registry
            .register(
                "hash1".to_string(),
                "ipfs".to_string(),
                "Qm1".to_string(),
                1024,
                1000,
            )
            .unwrap();

        registry.decref("hash1").unwrap();

        let orphans = gc_adapter::get_orphaned_hashes(&registry).unwrap();
        assert_eq!(orphans.len(), 1);
        assert!(orphans.contains(&"hash1".to_string()));
    }

    #[test]
    fn test_gc_adapter_duplicates() {
        let (registry, _temp) = setup_registry();

        registry
            .register(
                "hash1".to_string(),
                "ipfs".to_string(),
                "Qm1".to_string(),
                1024,
                1000,
            )
            .unwrap();
        registry
            .register(
                "hash1".to_string(),
                "filecoin".to_string(),
                "bafy1".to_string(),
                1024,
                1100,
            )
            .unwrap();

        let duplicates = gc_adapter::get_duplication_candidates(&registry).unwrap();
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].content_hash, "hash1");
        assert_eq!(duplicates[0].providers.len(), 2);
    }
}
