//! Reusable, stateless provenance validation for ingest admission control.
//!
//! Enforcement: Active. All store operations via `DataTieringOrchestrator` and
//! `ArchivePipeline` validate provenance chains when a `ProvenancePolicy` is set.
//! See `orchestrator::store_with_provenance_enforcement()` and
//! `archive_pipeline::archive_with_provenance()`.

use crate::types::{AdmissionDecision, ProvenanceChain, ProvenanceValidationResult};

/// Policy that governs what provenance chains are considered acceptable at ingest.
#[derive(Debug, Clone)]
pub struct ProvenancePolicy {
    /// If true, content without a provenance chain is rejected outright.
    pub require_provenance: bool,
    /// Minimum number of links required in the chain (0 = no minimum).
    pub min_chain_length: usize,
    /// Accepted signature algorithms. An empty list means all algorithms are allowed.
    pub allowed_algorithms: Vec<String>,
}

impl Default for ProvenancePolicy {
    fn default() -> Self {
        Self {
            require_provenance: true,
            min_chain_length: 1,
            allowed_algorithms: Vec::new(),
        }
    }
}

/// Stateless provenance validator that performs structural and policy checks.
pub struct ProvenanceValidator {
    policy: ProvenancePolicy,
}

impl ProvenanceValidator {
    /// Create a new validator with the given policy.
    pub fn new(policy: ProvenancePolicy) -> Self {
        Self { policy }
    }

    /// Structural validation: chain_id/root_hash non-empty, links non-empty,
    /// first link matches root_hash, parent_hash chain continuity,
    /// all link fields non-empty.
    pub fn validate_chain(&self, chain: &ProvenanceChain) -> ProvenanceValidationResult {
        if chain.chain_id.trim().is_empty() || chain.root_hash.trim().is_empty() {
            return ProvenanceValidationResult {
                valid: false,
                reason: "chain_id and root_hash are required".to_string(),
                invalid_links: chain.links.len(),
            };
        }

        if chain.links.is_empty() {
            return ProvenanceValidationResult {
                valid: false,
                reason: "at least one provenance link is required".to_string(),
                invalid_links: 1,
            };
        }

        let mut invalid_links = 0usize;
        for (idx, link) in chain.links.iter().enumerate() {
            if link.artifact_hash.trim().is_empty()
                || link.signer.trim().is_empty()
                || link.signature.trim().is_empty()
                || link.algorithm.trim().is_empty()
            {
                invalid_links += 1;
                continue;
            }

            if idx == 0 {
                if link.artifact_hash != chain.root_hash {
                    invalid_links += 1;
                }
            } else {
                let prev = &chain.links[idx - 1];
                if link.parent_hash.as_deref() != Some(prev.artifact_hash.as_str()) {
                    invalid_links += 1;
                }
            }
        }

        if invalid_links > 0 {
            return ProvenanceValidationResult {
                valid: false,
                reason: "provenance chain continuity/signature metadata failed validation"
                    .to_string(),
                invalid_links,
            };
        }

        ProvenanceValidationResult {
            valid: true,
            reason: "valid provenance chain".to_string(),
            invalid_links: 0,
        }
    }

    /// Policy validation: min_chain_length, allowed_algorithms.
    ///
    /// Returns `Ok(())` when the chain satisfies all policy constraints,
    /// otherwise `Err` with a human-readable reason.
    pub fn validate_with_policy(&self, chain: &ProvenanceChain) -> Result<(), String> {
        // Structural integrity first.
        let result = self.validate_chain(chain);
        if !result.valid {
            return Err(result.reason);
        }

        // Minimum chain length.
        if chain.links.len() < self.policy.min_chain_length {
            return Err(format!(
                "provenance chain has {} links but policy requires at least {}",
                chain.links.len(),
                self.policy.min_chain_length
            ));
        }

        // Allowed algorithms (empty list = allow all).
        if !self.policy.allowed_algorithms.is_empty() {
            for link in &chain.links {
                if !self.policy.allowed_algorithms.contains(&link.algorithm) {
                    return Err(format!(
                        "algorithm '{}' is not in the allowed algorithms list",
                        link.algorithm
                    ));
                }
            }
        }

        Ok(())
    }

    /// Full admission check combining structural + policy validation.
    ///
    /// * `None` provenance -> Rejected (if required) or Accepted (if optional).
    /// * Invalid chain -> Quarantined with reason.
    /// * Valid chain -> Accepted.
    pub fn decide_admission(
        &self,
        chain: Option<&ProvenanceChain>,
    ) -> (AdmissionDecision, Option<String>) {
        let chain = match chain {
            Some(c) => c,
            None => {
                if self.policy.require_provenance {
                    return (AdmissionDecision::Rejected, None);
                }
                return (AdmissionDecision::Accepted, None);
            }
        };

        match self.validate_with_policy(chain) {
            Ok(()) => (AdmissionDecision::Accepted, None),
            Err(reason) => (AdmissionDecision::Quarantined, Some(reason)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ProvenanceLink;

    fn make_valid_chain() -> ProvenanceChain {
        ProvenanceChain {
            chain_id: "chain-1".to_string(),
            root_hash: "root".to_string(),
            links: vec![
                ProvenanceLink {
                    artifact_hash: "root".to_string(),
                    parent_hash: None,
                    signer: "signer-1".to_string(),
                    signature: "sig-1".to_string(),
                    algorithm: "ed25519".to_string(),
                    signed_at: 10,
                },
                ProvenanceLink {
                    artifact_hash: "child-1".to_string(),
                    parent_hash: Some("root".to_string()),
                    signer: "signer-2".to_string(),
                    signature: "sig-2".to_string(),
                    algorithm: "ed25519".to_string(),
                    signed_at: 11,
                },
            ],
        }
    }

    #[test]
    fn test_valid_chain_accepted() {
        let validator = ProvenanceValidator::new(ProvenancePolicy::default());
        let (decision, reason) = validator.decide_admission(Some(&make_valid_chain()));
        assert_eq!(decision, AdmissionDecision::Accepted);
        assert!(reason.is_none());
    }

    #[test]
    fn test_missing_provenance_rejected() {
        let validator = ProvenanceValidator::new(ProvenancePolicy {
            require_provenance: true,
            ..ProvenancePolicy::default()
        });
        let (decision, _) = validator.decide_admission(None);
        assert_eq!(decision, AdmissionDecision::Rejected);
    }

    #[test]
    fn test_missing_provenance_accepted_when_optional() {
        let validator = ProvenanceValidator::new(ProvenancePolicy {
            require_provenance: false,
            ..ProvenancePolicy::default()
        });
        let (decision, _) = validator.decide_admission(None);
        assert_eq!(decision, AdmissionDecision::Accepted);
    }

    #[test]
    fn test_invalid_chain_quarantined() {
        let validator = ProvenanceValidator::new(ProvenancePolicy::default());
        let mut chain = make_valid_chain();
        // Break root_hash match — first link artifact_hash won't match
        chain.root_hash = "different-root".to_string();
        let (decision, reason) = validator.decide_admission(Some(&chain));
        assert_eq!(decision, AdmissionDecision::Quarantined);
        assert!(reason.is_some());
    }

    #[test]
    fn test_empty_fields_quarantined() {
        let validator = ProvenanceValidator::new(ProvenancePolicy::default());
        let chain = ProvenanceChain {
            chain_id: "chain-1".to_string(),
            root_hash: "root".to_string(),
            links: vec![ProvenanceLink {
                artifact_hash: "root".to_string(),
                parent_hash: None,
                signer: "".to_string(),    // blank signer
                signature: "".to_string(), // blank signature
                algorithm: "ed25519".to_string(),
                signed_at: 10,
            }],
        };
        let (decision, reason) = validator.decide_admission(Some(&chain));
        assert_eq!(decision, AdmissionDecision::Quarantined);
        assert!(reason.is_some());
    }

    #[test]
    fn test_algorithm_filtering() {
        let validator = ProvenanceValidator::new(ProvenancePolicy {
            require_provenance: true,
            min_chain_length: 1,
            allowed_algorithms: vec!["secp256k1".to_string()],
        });
        // make_valid_chain uses "ed25519" which is not in allowed list
        let (decision, reason) = validator.decide_admission(Some(&make_valid_chain()));
        assert_eq!(decision, AdmissionDecision::Quarantined);
        assert!(reason.unwrap().contains("algorithm"));
    }

    #[test]
    fn test_min_chain_length_enforcement() {
        let validator = ProvenanceValidator::new(ProvenancePolicy {
            require_provenance: true,
            min_chain_length: 5, // valid chain only has 2 links
            allowed_algorithms: Vec::new(),
        });
        let (decision, reason) = validator.decide_admission(Some(&make_valid_chain()));
        assert_eq!(decision, AdmissionDecision::Quarantined);
        assert!(reason.unwrap().contains("links"));
    }
}
