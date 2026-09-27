use std::collections::HashMap;

use crate::types::{ContentId, EnvironmentalDomain};

/// Metadata stored alongside each indexed content entry.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub content_id: ContentId,
    pub domain: EnvironmentalDomain,
    pub station_id: String,
    pub report_id: String,
    pub stored_at: u64,
}

/// In-memory index mapping environmental domains to stored content IDs.
pub struct DomainContentIndex {
    by_domain: HashMap<EnvironmentalDomain, Vec<IndexEntry>>,
    by_content_id: HashMap<String, IndexEntry>,
}

impl DomainContentIndex {
    /// Creates an empty index.
    pub fn new() -> Self {
        Self {
            by_domain: HashMap::new(),
            by_content_id: HashMap::new(),
        }
    }

    /// Inserts an entry into both the domain and content-ID maps.
    pub fn insert(&mut self, entry: IndexEntry) {
        let key = entry.content_id.to_string();
        self.by_domain
            .entry(entry.domain)
            .or_default()
            .push(entry.clone());
        self.by_content_id.insert(key, entry);
    }

    /// Returns all entries for the given domain, or an empty slice if none exist.
    pub fn query_by_domain(&self, domain: &EnvironmentalDomain) -> &[IndexEntry] {
        self.by_domain
            .get(domain)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Looks up a single entry by its content ID string (e.g. `"ipfs:abc"`).
    pub fn get_by_content_id(&self, content_id_str: &str) -> Option<&IndexEntry> {
        self.by_content_id.get(content_id_str)
    }

    /// Returns the number of entries stored under the given domain.
    pub fn count_by_domain(&self, domain: &EnvironmentalDomain) -> usize {
        self.by_domain.get(domain).map(|v| v.len()).unwrap_or(0)
    }

    /// Returns the total number of indexed entries.
    pub fn total_count(&self) -> usize {
        self.by_content_id.len()
    }

    /// Removes an entry by content ID string. Returns `true` if found and removed.
    pub fn remove(&mut self, content_id_str: &str) -> bool {
        if let Some(entry) = self.by_content_id.remove(content_id_str) {
            if let Some(entries) = self.by_domain.get_mut(&entry.domain) {
                entries.retain(|e| e.content_id.to_string() != content_id_str);
            }
            true
        } else {
            false
        }
    }
}

impl Default for DomainContentIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soil_entry(hash: &str) -> IndexEntry {
        IndexEntry {
            content_id: ContentId {
                provider: "ipfs".to_string(),
                hash: hash.to_string(),
            },
            domain: EnvironmentalDomain::Soil,
            station_id: "station-soil-1".to_string(),
            report_id: "report-soil-1".to_string(),
            stored_at: 1000,
        }
    }

    fn ocean_entry(hash: &str) -> IndexEntry {
        IndexEntry {
            content_id: ContentId {
                provider: "ipfs".to_string(),
                hash: hash.to_string(),
            },
            domain: EnvironmentalDomain::Ocean,
            station_id: "station-ocean-1".to_string(),
            report_id: "report-ocean-1".to_string(),
            stored_at: 2000,
        }
    }

    #[test]
    fn test_insert_and_query_by_domain() {
        let mut index = DomainContentIndex::new();
        index.insert(soil_entry("aaa"));
        index.insert(soil_entry("bbb"));
        index.insert(ocean_entry("ccc"));

        assert_eq!(index.query_by_domain(&EnvironmentalDomain::Soil).len(), 2);
        assert_eq!(index.query_by_domain(&EnvironmentalDomain::Ocean).len(), 1);
        assert_eq!(
            index.query_by_domain(&EnvironmentalDomain::Hydrology).len(),
            0
        );
    }

    #[test]
    fn test_get_by_content_id() {
        let mut index = DomainContentIndex::new();
        index.insert(soil_entry("abc"));

        let entry = index.get_by_content_id("ipfs:abc");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().station_id, "station-soil-1");
    }

    #[test]
    fn test_count_by_domain() {
        let mut index = DomainContentIndex::new();
        index.insert(soil_entry("aaa"));
        index.insert(soil_entry("bbb"));

        assert_eq!(index.count_by_domain(&EnvironmentalDomain::Soil), 2);
        assert_eq!(index.count_by_domain(&EnvironmentalDomain::Ocean), 0);
    }

    #[test]
    fn test_total_count() {
        let mut index = DomainContentIndex::new();
        index.insert(soil_entry("aaa"));
        index.insert(ocean_entry("bbb"));

        assert_eq!(index.total_count(), 2);
    }

    #[test]
    fn test_remove() {
        let mut index = DomainContentIndex::new();
        index.insert(soil_entry("aaa"));

        assert!(index.remove("ipfs:aaa"));
        assert_eq!(index.total_count(), 0);
        assert_eq!(index.count_by_domain(&EnvironmentalDomain::Soil), 0);
        assert!(!index.remove("ipfs:aaa"));
    }

    #[test]
    fn test_query_empty_domain() {
        let index = DomainContentIndex::new();
        let results = index.query_by_domain(&EnvironmentalDomain::Wildfire);
        assert!(results.is_empty());
    }
}
