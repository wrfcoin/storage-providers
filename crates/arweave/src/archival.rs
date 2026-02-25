use serde::{Deserialize, Serialize};
use wrfcoin_storage_common::Metadata;

/// How long content should be retained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetentionPeriod {
    /// Retain for a specific number of days.
    Days(u64),
    /// Retain for a specific number of years.
    Years(u64),
    /// Retain permanently (Arweave's native guarantee).
    Permanent,
}

/// Level of redundancy for archived content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RedundancyLevel {
    /// Single copy, minimal redundancy.
    Basic,
    /// Standard redundancy (multiple provider replicas).
    Standard,
    /// High redundancy for important data.
    High,
    /// Critical-level redundancy for irreplaceable records.
    Critical,
}

/// Policy governing whether and how content should be archived.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivalPolicy {
    /// How long to retain the content.
    pub retention: RetentionPeriod,
    /// Desired redundancy level.
    pub redundancy: RedundancyLevel,
    /// Minimum data size in bytes to qualify for archival.
    pub min_size: u64,
    /// Required tag key that must be present for archival.
    pub required_tag: Option<String>,
}

/// Manages archival policies and decides whether content should be archived.
#[derive(Debug)]
pub struct ArchivalManager {
    policies: Vec<ArchivalPolicy>,
}

impl ArchivalManager {
    /// Create a new archival manager with no policies.
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    /// Add an archival policy.
    pub fn add_policy(&mut self, policy: ArchivalPolicy) {
        self.policies.push(policy);
    }

    /// Return all configured policies.
    pub fn policies(&self) -> &[ArchivalPolicy] {
        &self.policies
    }

    /// Determine whether the given metadata qualifies for archival under any policy.
    pub fn should_archive(&self, metadata: &Metadata) -> bool {
        self.policies.iter().any(|policy| {
            // Check minimum size.
            if metadata.size < policy.min_size {
                return false;
            }

            // Check required tag.
            if let Some(ref tag_key) = policy.required_tag {
                if !metadata.tags.contains_key(tag_key) {
                    return false;
                }
            }

            true
        })
    }
}

impl Default for ArchivalManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn meta(size: u64, tags: Vec<(&str, &str)>) -> Metadata {
        let mut tag_map = HashMap::new();
        for (k, v) in tags {
            tag_map.insert(k.to_string(), v.to_string());
        }
        Metadata {
            content_type: "application/json".to_string(),
            size,
            timestamp: 1000,
            tags: tag_map,
        }
    }

    #[test]
    fn test_add_policy() {
        let mut am = ArchivalManager::new();
        am.add_policy(ArchivalPolicy {
            retention: RetentionPeriod::Permanent,
            redundancy: RedundancyLevel::High,
            min_size: 0,
            required_tag: None,
        });
        assert_eq!(am.policies().len(), 1);
    }

    #[test]
    fn test_should_archive_size_threshold() {
        let mut am = ArchivalManager::new();
        am.add_policy(ArchivalPolicy {
            retention: RetentionPeriod::Permanent,
            redundancy: RedundancyLevel::Standard,
            min_size: 1024,
            required_tag: None,
        });
        assert!(!am.should_archive(&meta(500, vec![])));
        assert!(am.should_archive(&meta(1024, vec![])));
        assert!(am.should_archive(&meta(2048, vec![])));
    }

    #[test]
    fn test_should_archive_required_tag() {
        let mut am = ArchivalManager::new();
        am.add_policy(ArchivalPolicy {
            retention: RetentionPeriod::Years(10),
            redundancy: RedundancyLevel::Critical,
            min_size: 0,
            required_tag: Some("weather".to_string()),
        });
        assert!(!am.should_archive(&meta(100, vec![])));
        assert!(am.should_archive(&meta(100, vec![("weather", "true")])));
    }

    #[test]
    fn test_should_archive_no_policies() {
        let am = ArchivalManager::new();
        assert!(!am.should_archive(&meta(1000, vec![])));
    }

    #[test]
    fn test_multiple_policies_any_match() {
        let mut am = ArchivalManager::new();
        am.add_policy(ArchivalPolicy {
            retention: RetentionPeriod::Permanent,
            redundancy: RedundancyLevel::Basic,
            min_size: 10_000, // High threshold
            required_tag: None,
        });
        am.add_policy(ArchivalPolicy {
            retention: RetentionPeriod::Days(30),
            redundancy: RedundancyLevel::Standard,
            min_size: 0, // Low threshold
            required_tag: None,
        });
        // Matches second policy even though first doesn't match
        assert!(am.should_archive(&meta(100, vec![])));
    }
}
