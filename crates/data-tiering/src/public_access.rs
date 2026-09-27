use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wrfcoin_storage_common::DataTier;

use crate::quality::TrustClass;

/// Classification of report access level for public-interest content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AccessTier {
    /// Open to all — no authentication required.
    OpenAccess,
    /// Registered users only (any trust class).
    RegisteredAccess,
    /// Restricted to Verified and Standard trust classes.
    RestrictedAccess,
    /// Internal only — governance review.
    InternalOnly,
}

/// Policy defining which report classes get open access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicInterestAccessPolicy {
    /// The report class this policy applies to (e.g. "public_interest.weather").
    pub report_class: String,
    /// The access tier granted to this report class.
    pub access_tier: AccessTier,
    /// Minimum quality score required for access (inclusive).
    pub min_quality_score: f64,
    /// Whether provenance attestation is required for access.
    pub requires_provenance: bool,
    /// The maximum (least warm) storage tier allowed; content must be at this
    /// tier or warmer (Hot ≤ Warm ≤ Cold ≤ Archive).
    pub max_storage_tier: DataTier,
}

/// Result of an access check.
#[derive(Debug, Clone)]
pub struct AccessCheckResult {
    /// Whether access is permitted.
    pub allowed: bool,
    /// Human-readable explanation.
    pub reason: String,
    /// The access tier that governed the decision.
    pub effective_tier: AccessTier,
}

/// Returns a numeric warmth rank for `DataTier`.
///
/// Lower numbers are warmer (Hot = 0, Archive = 3).
fn tier_rank(tier: &DataTier) -> u8 {
    match tier {
        DataTier::Hot => 0,
        DataTier::Warm => 1,
        DataTier::Cold => 2,
        DataTier::Archive => 3,
    }
}

/// Returns `true` when `actual` is at least as warm as `required`.
///
/// `Hot < Warm < Cold < Archive` (Hot is warmest).  "At least as warm"
/// means `tier_rank(actual) <= tier_rank(required)`.
#[cfg(test)]
fn is_warm_enough(actual: &DataTier, required: &DataTier) -> bool {
    tier_rank(actual) <= tier_rank(required)
}

/// Manages public-interest access enforcement.
///
/// # Governance Overrides
///
/// The manager supports governance-driven overrides that temporarily grant `OpenAccess`
/// to specific report classes, superseding any registered policy. This allows governance
/// bodies to enforce open-access tiers when the standard policy would otherwise restrict
/// access (e.g., during emergency situations or regulatory mandates).
///
/// ## Override Semantics
/// - When a governance override is active for a report class, access checks return
///   `AccessCheckResult { allowed: true, reason: "governance override active", ... }`
///   for all requesters, regardless of trust class, quality score, or provenance status.
/// - Overrides do not modify the underlying policies — they are temporary and must
///   be explicitly cleared via [`clear_governance_override`](Self::clear_governance_override).
/// - Overrides take precedence over all other access controls.
///
/// ## Usage Example
/// ```ignore
/// let mut mgr = PublicInterestAccessManager::new();
/// mgr.register_policy(/* restricted policy */);
///
/// // Governance decision to open access during emergency
/// mgr.set_governance_override("weather.emergency");
///
/// // Now all requesters can access weather.emergency, regardless of policy
/// let result = mgr.check_access("weather.emergency", &TrustClass::Untrusted, 0.0, false, &DataTier::Archive);
/// assert!(result.allowed);
///
/// // Clear override when conditions change
/// mgr.clear_governance_override("weather.emergency");
/// ```
pub struct PublicInterestAccessManager {
    policies: HashMap<String, PublicInterestAccessPolicy>,
    /// Default access tier for report classes without an explicit policy.
    default_tier: AccessTier,
    /// Report classes where governance has mandated OpenAccess.
    governance_overrides: HashMap<String, bool>,
}

impl PublicInterestAccessManager {
    /// Create a new manager with `RegisteredAccess` as the default tier and
    /// no pre-registered policies or governance overrides.
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            default_tier: AccessTier::RegisteredAccess,
            governance_overrides: HashMap::new(),
        }
    }

    /// Register (or replace) a policy for the report class it targets.
    pub fn register_policy(&mut self, policy: PublicInterestAccessPolicy) {
        self.policies.insert(policy.report_class.clone(), policy);
    }

    /// Remove the policy for `report_class`.
    ///
    /// Returns `true` if a policy was present and removed, `false` otherwise.
    pub fn remove_policy(&mut self, report_class: &str) -> bool {
        self.policies.remove(report_class).is_some()
    }

    /// Check whether a requester may read a report.
    ///
    /// # Governance Override
    ///
    /// If a governance override is active for `report_class`, access is immediately
    /// granted with `allowed: true` and `reason: "governance override active"`,
    /// bypassing all other checks.
    ///
    /// # Access-tier rules
    ///
    /// | Tier               | Allowed trust classes                  |
    /// |--------------------|----------------------------------------|
    /// | `OpenAccess`       | All (including `Untrusted`)            |
    /// | `RegisteredAccess` | All except `Untrusted`                 |
    /// | `RestrictedAccess` | `Verified` and `Standard` only         |
    /// | `InternalOnly`     | `Verified` only                        |
    ///
    /// Additionally:
    /// - `quality_score` must be ≥ the policy's `min_quality_score`.
    /// - `has_provenance` must be `true` when the policy's `requires_provenance` is `true`.
    /// - `storage_tier` must be at least as warm as the policy's `max_storage_tier`.
    ///
    /// When no policy is registered for `report_class`, the manager's
    /// `default_tier` (`RegisteredAccess`) governs the trust-class check,
    /// and quality / provenance / storage constraints are not applied.
    pub fn check_access(
        &self,
        report_class: &str,
        requester_trust: &TrustClass,
        quality_score: f64,
        has_provenance: bool,
        storage_tier: &DataTier,
    ) -> AccessCheckResult {
        // --- Governance override (highest precedence) ---
        if self.governance_overrides.contains_key(report_class) {
            return AccessCheckResult {
                allowed: true,
                reason: "governance override active".to_string(),
                effective_tier: AccessTier::OpenAccess,
            };
        }

        // Resolve the effective policy, or fall back to the default tier.
        let (effective_tier, policy_opt) = match self.policies.get(report_class) {
            Some(p) => (p.access_tier.clone(), Some(p)),
            None => (self.default_tier.clone(), None),
        };

        // --- Trust-class gate ---
        let trust_allowed = match &effective_tier {
            AccessTier::OpenAccess => true,
            AccessTier::RegisteredAccess => !matches!(requester_trust, TrustClass::Untrusted),
            AccessTier::RestrictedAccess => {
                matches!(requester_trust, TrustClass::Verified | TrustClass::Standard)
            }
            AccessTier::InternalOnly => matches!(requester_trust, TrustClass::Verified),
        };

        if !trust_allowed {
            return AccessCheckResult {
                allowed: false,
                reason: format!(
                    "trust class {:?} is not permitted for {:?} access tier",
                    requester_trust, effective_tier
                ),
                effective_tier,
            };
        }

        // --- Additional policy constraints (only when an explicit policy exists) ---
        if let Some(policy) = policy_opt {
            if quality_score < policy.min_quality_score {
                return AccessCheckResult {
                    allowed: false,
                    reason: format!(
                        "quality score {:.4} is below minimum {:.4}",
                        quality_score, policy.min_quality_score
                    ),
                    effective_tier,
                };
            }

            if policy.requires_provenance && !has_provenance {
                return AccessCheckResult {
                    allowed: false,
                    reason: "provenance attestation is required but not present".to_string(),
                    effective_tier,
                };
            }

            // Actual tier must be at most as cold as max_storage_tier.
            // tier_rank: Hot=0, Warm=1, Cold=2, Archive=3 (lower = warmer).
            // If actual has a higher rank than max_storage_tier, it is colder → reject.
            if tier_rank(storage_tier) > tier_rank(&policy.max_storage_tier) {
                return AccessCheckResult {
                    allowed: false,
                    reason: format!(
                        "storage tier {:?} is colder than the required maximum {:?}",
                        storage_tier, policy.max_storage_tier
                    ),
                    effective_tier,
                };
            }
        }

        AccessCheckResult {
            allowed: true,
            reason: "access granted".to_string(),
            effective_tier,
        }
    }

    /// Look up the policy for `report_class`.
    pub fn get_policy(&self, report_class: &str) -> Option<&PublicInterestAccessPolicy> {
        self.policies.get(report_class)
    }

    /// Return the names of all report classes whose policy has `OpenAccess`.
    pub fn list_open_access_classes(&self) -> Vec<String> {
        let mut classes: Vec<String> = self
            .policies
            .values()
            .filter(|p| p.access_tier == AccessTier::OpenAccess)
            .map(|p| p.report_class.clone())
            .collect();
        classes.sort(); // deterministic ordering
        classes
    }

    /// Return the number of registered policies.
    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }

    // --- Governance Override Hooks ---

    /// Set a governance override for `report_class`, granting `OpenAccess` to all requesters.
    ///
    /// This method records a governance decision to override the standard access policy
    /// for the given report class, immediately granting `OpenAccess` to all users
    /// (including unauthenticated readers). The override persists until explicitly
    /// cleared with [`clear_governance_override`](Self::clear_governance_override).
    ///
    /// # Arguments
    /// * `report_class` - The report class to override (e.g., "weather.emergency")
    ///
    /// # Notes
    /// - Overrides take precedence over all registered policies.
    /// - Overrides do not modify the underlying policy — they are temporary governance directives.
    /// - Multiple overlapping overrides are allowed (e.g., both "weather" and "weather.emergency" can be overridden).
    pub fn set_governance_override(&mut self, report_class: &str) {
        self.governance_overrides
            .insert(report_class.to_string(), true);
    }

    /// Clear a governance override for `report_class`, reverting to the standard policy.
    ///
    /// This method removes a governance override, allowing the standard access policy
    /// (or default tier) to govern access again. If no override exists for the given
    /// report class, this method is a no-op and returns `false`.
    ///
    /// # Arguments
    /// * `report_class` - The report class whose override should be cleared
    ///
    /// # Returns
    /// `true` if an override was present and removed, `false` otherwise.
    pub fn clear_governance_override(&mut self, report_class: &str) -> bool {
        self.governance_overrides.remove(report_class).is_some()
    }

    /// Return a list of all report classes that have active governance overrides.
    ///
    /// # Returns
    /// A vector of report class names sorted in alphabetical order.
    pub fn list_governance_overrides(&self) -> Vec<String> {
        let mut classes: Vec<String> = self.governance_overrides.keys().cloned().collect();
        classes.sort();
        classes
    }

    /// Check whether a governance override is active for the given report class.
    pub fn has_governance_override(&self, report_class: &str) -> bool {
        self.governance_overrides.contains_key(report_class)
    }
}

impl Default for PublicInterestAccessManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_access_policy(report_class: &str) -> PublicInterestAccessPolicy {
        PublicInterestAccessPolicy {
            report_class: report_class.to_string(),
            access_tier: AccessTier::OpenAccess,
            min_quality_score: 0.0,
            requires_provenance: false,
            max_storage_tier: DataTier::Archive,
        }
    }

    fn registered_access_policy(report_class: &str) -> PublicInterestAccessPolicy {
        PublicInterestAccessPolicy {
            report_class: report_class.to_string(),
            access_tier: AccessTier::RegisteredAccess,
            min_quality_score: 0.0,
            requires_provenance: false,
            max_storage_tier: DataTier::Archive,
        }
    }

    fn restricted_access_policy(report_class: &str) -> PublicInterestAccessPolicy {
        PublicInterestAccessPolicy {
            report_class: report_class.to_string(),
            access_tier: AccessTier::RestrictedAccess,
            min_quality_score: 0.0,
            requires_provenance: false,
            max_storage_tier: DataTier::Archive,
        }
    }

    fn internal_only_policy(report_class: &str) -> PublicInterestAccessPolicy {
        PublicInterestAccessPolicy {
            report_class: report_class.to_string(),
            access_tier: AccessTier::InternalOnly,
            min_quality_score: 0.0,
            requires_provenance: false,
            max_storage_tier: DataTier::Archive,
        }
    }

    // 1. Default manager has RegisteredAccess default tier
    #[test]
    fn test_default_manager_has_registered_access_default_tier() {
        let mgr = PublicInterestAccessManager::new();
        assert_eq!(mgr.default_tier, AccessTier::RegisteredAccess);
    }

    // 2. Register and retrieve policy
    #[test]
    fn test_register_and_retrieve_policy() {
        let mut mgr = PublicInterestAccessManager::new();
        let policy = open_access_policy("public_interest.weather");
        mgr.register_policy(policy);
        let retrieved = mgr.get_policy("public_interest.weather").unwrap();
        assert_eq!(retrieved.report_class, "public_interest.weather");
        assert_eq!(retrieved.access_tier, AccessTier::OpenAccess);
    }

    // 3. OpenAccess allows all trust classes including Untrusted
    #[test]
    fn test_open_access_allows_all_trust_classes_including_untrusted() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(open_access_policy("weather.open"));

        for trust in [
            TrustClass::Verified,
            TrustClass::Standard,
            TrustClass::Probation,
            TrustClass::Untrusted,
        ] {
            let result = mgr.check_access("weather.open", &trust, 0.0, false, &DataTier::Archive);
            assert!(
                result.allowed,
                "OpenAccess should allow {:?}, but got: {}",
                trust, result.reason
            );
            assert_eq!(result.effective_tier, AccessTier::OpenAccess);
        }
    }

    // 4. RegisteredAccess blocks Untrusted
    #[test]
    fn test_registered_access_blocks_untrusted() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(registered_access_policy("weather.registered"));

        // Untrusted must be blocked
        let blocked = mgr.check_access(
            "weather.registered",
            &TrustClass::Untrusted,
            0.0,
            false,
            &DataTier::Archive,
        );
        assert!(!blocked.allowed);

        // All other trust classes must be allowed
        for trust in [
            TrustClass::Verified,
            TrustClass::Standard,
            TrustClass::Probation,
        ] {
            let result =
                mgr.check_access("weather.registered", &trust, 0.0, false, &DataTier::Archive);
            assert!(
                result.allowed,
                "RegisteredAccess should allow {:?}, but got: {}",
                trust, result.reason
            );
        }
    }

    // 5. RestrictedAccess only allows Verified and Standard
    #[test]
    fn test_restricted_access_only_allows_verified_and_standard() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(restricted_access_policy("weather.restricted"));

        for trust in [TrustClass::Verified, TrustClass::Standard] {
            let result =
                mgr.check_access("weather.restricted", &trust, 0.0, false, &DataTier::Archive);
            assert!(
                result.allowed,
                "RestrictedAccess should allow {:?}, but got: {}",
                trust, result.reason
            );
        }

        for trust in [TrustClass::Probation, TrustClass::Untrusted] {
            let result =
                mgr.check_access("weather.restricted", &trust, 0.0, false, &DataTier::Archive);
            assert!(!result.allowed, "RestrictedAccess should deny {:?}", trust);
        }
    }

    // 6. InternalOnly only allows Verified
    #[test]
    fn test_internal_only_allows_only_verified() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(internal_only_policy("weather.internal"));

        let allowed = mgr.check_access(
            "weather.internal",
            &TrustClass::Verified,
            0.0,
            false,
            &DataTier::Archive,
        );
        assert!(allowed.allowed);

        for trust in [
            TrustClass::Standard,
            TrustClass::Probation,
            TrustClass::Untrusted,
        ] {
            let result =
                mgr.check_access("weather.internal", &trust, 0.0, false, &DataTier::Archive);
            assert!(!result.allowed, "InternalOnly should deny {:?}", trust);
        }
    }

    // 7. Quality score below minimum rejects access
    #[test]
    fn test_quality_score_below_minimum_rejects_access() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(PublicInterestAccessPolicy {
            report_class: "weather.quality".to_string(),
            access_tier: AccessTier::OpenAccess,
            min_quality_score: 0.8,
            requires_provenance: false,
            max_storage_tier: DataTier::Archive,
        });

        // Below threshold — rejected
        let result = mgr.check_access(
            "weather.quality",
            &TrustClass::Verified,
            0.79,
            false,
            &DataTier::Archive,
        );
        assert!(!result.allowed);
        assert!(result.reason.contains("quality score"));

        // At threshold — allowed
        let result = mgr.check_access(
            "weather.quality",
            &TrustClass::Verified,
            0.8,
            false,
            &DataTier::Archive,
        );
        assert!(result.allowed);
    }

    // 8. Missing provenance rejects when required
    #[test]
    fn test_missing_provenance_rejects_when_required() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(PublicInterestAccessPolicy {
            report_class: "weather.provenance".to_string(),
            access_tier: AccessTier::OpenAccess,
            min_quality_score: 0.0,
            requires_provenance: true,
            max_storage_tier: DataTier::Archive,
        });

        // No provenance — rejected
        let result = mgr.check_access(
            "weather.provenance",
            &TrustClass::Verified,
            1.0,
            false,
            &DataTier::Archive,
        );
        assert!(!result.allowed);
        assert!(result.reason.contains("provenance"));

        // With provenance — allowed
        let result = mgr.check_access(
            "weather.provenance",
            &TrustClass::Verified,
            1.0,
            true,
            &DataTier::Archive,
        );
        assert!(result.allowed);
    }

    // 9. Storage tier too cold rejects access
    #[test]
    fn test_storage_tier_too_cold_rejects_access() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(PublicInterestAccessPolicy {
            report_class: "weather.tier".to_string(),
            access_tier: AccessTier::OpenAccess,
            min_quality_score: 0.0,
            requires_provenance: false,
            max_storage_tier: DataTier::Warm, // content must be Hot or Warm
        });

        // Cold is colder than Warm — rejected
        let result = mgr.check_access(
            "weather.tier",
            &TrustClass::Verified,
            1.0,
            false,
            &DataTier::Cold,
        );
        assert!(!result.allowed);
        assert!(result.reason.contains("colder"));

        // Archive is colder than Warm — rejected
        let result = mgr.check_access(
            "weather.tier",
            &TrustClass::Verified,
            1.0,
            false,
            &DataTier::Archive,
        );
        assert!(!result.allowed);

        // Hot is warmer than Warm — allowed
        let result = mgr.check_access(
            "weather.tier",
            &TrustClass::Verified,
            1.0,
            false,
            &DataTier::Hot,
        );
        assert!(result.allowed);

        // Warm equals max — allowed
        let result = mgr.check_access(
            "weather.tier",
            &TrustClass::Verified,
            1.0,
            false,
            &DataTier::Warm,
        );
        assert!(result.allowed);
    }

    // 10. Remove policy works
    #[test]
    fn test_remove_policy_works() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(open_access_policy("weather.remove_me"));
        assert_eq!(mgr.policy_count(), 1);

        let removed = mgr.remove_policy("weather.remove_me");
        assert!(removed);
        assert_eq!(mgr.policy_count(), 0);
        assert!(mgr.get_policy("weather.remove_me").is_none());

        // Removing a non-existent policy returns false
        let not_removed = mgr.remove_policy("weather.remove_me");
        assert!(!not_removed);
    }

    // 11. list_open_access_classes returns correct set
    #[test]
    fn test_list_open_access_classes_returns_correct_set() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(open_access_policy("weather.open1"));
        mgr.register_policy(open_access_policy("weather.open2"));
        mgr.register_policy(registered_access_policy("weather.registered"));
        mgr.register_policy(internal_only_policy("weather.internal"));

        let open_classes = mgr.list_open_access_classes();
        assert_eq!(open_classes.len(), 2);
        assert!(open_classes.contains(&"weather.open1".to_string()));
        assert!(open_classes.contains(&"weather.open2".to_string()));
        assert!(!open_classes.contains(&"weather.registered".to_string()));
        assert!(!open_classes.contains(&"weather.internal".to_string()));
    }

    // 12. Unknown report class uses default tier (RegisteredAccess)
    #[test]
    fn test_unknown_report_class_uses_default_tier() {
        let mgr = PublicInterestAccessManager::new();

        // Untrusted should be denied (RegisteredAccess default blocks Untrusted)
        let denied = mgr.check_access(
            "unknown.class",
            &TrustClass::Untrusted,
            0.0,
            false,
            &DataTier::Archive,
        );
        assert!(!denied.allowed);
        assert_eq!(denied.effective_tier, AccessTier::RegisteredAccess);

        // Verified should be allowed
        let allowed = mgr.check_access(
            "unknown.class",
            &TrustClass::Verified,
            0.0,
            false,
            &DataTier::Archive,
        );
        assert!(allowed.allowed);
        assert_eq!(allowed.effective_tier, AccessTier::RegisteredAccess);
    }

    // Additional: serde round-trip for AccessTier and PublicInterestAccessPolicy
    #[test]
    fn test_access_tier_serde_roundtrip() {
        for tier in [
            AccessTier::OpenAccess,
            AccessTier::RegisteredAccess,
            AccessTier::RestrictedAccess,
            AccessTier::InternalOnly,
        ] {
            let json = serde_json::to_string(&tier).expect("serialize");
            let decoded: AccessTier = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(decoded, tier);
        }
    }

    #[test]
    fn test_public_interest_access_policy_serde_roundtrip() {
        let policy = PublicInterestAccessPolicy {
            report_class: "weather.test".to_string(),
            access_tier: AccessTier::RestrictedAccess,
            min_quality_score: 0.75,
            requires_provenance: true,
            max_storage_tier: DataTier::Cold,
        };
        let json = serde_json::to_string(&policy).expect("serialize");
        let decoded: PublicInterestAccessPolicy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.report_class, policy.report_class);
        assert_eq!(decoded.access_tier, AccessTier::RestrictedAccess);
        assert!((decoded.min_quality_score - 0.75).abs() < f64::EPSILON);
        assert!(decoded.requires_provenance);
        assert_eq!(decoded.max_storage_tier, DataTier::Cold);
    }

    #[test]
    fn test_policy_count() {
        let mut mgr = PublicInterestAccessManager::new();
        assert_eq!(mgr.policy_count(), 0);
        mgr.register_policy(open_access_policy("a"));
        assert_eq!(mgr.policy_count(), 1);
        mgr.register_policy(open_access_policy("b"));
        assert_eq!(mgr.policy_count(), 2);
        mgr.remove_policy("a");
        assert_eq!(mgr.policy_count(), 1);
    }

    #[test]
    fn test_register_policy_replaces_existing() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(open_access_policy("weather.replace"));
        mgr.register_policy(internal_only_policy("weather.replace"));
        assert_eq!(mgr.policy_count(), 1);
        assert_eq!(
            mgr.get_policy("weather.replace").unwrap().access_tier,
            AccessTier::InternalOnly
        );
    }

    #[test]
    fn test_tier_warmth_ordering() {
        // Verify tier_rank ordering: Hot < Warm < Cold < Archive
        assert!(tier_rank(&DataTier::Hot) < tier_rank(&DataTier::Warm));
        assert!(tier_rank(&DataTier::Warm) < tier_rank(&DataTier::Cold));
        assert!(tier_rank(&DataTier::Cold) < tier_rank(&DataTier::Archive));

        // is_warm_enough: actual at or warmer than required
        assert!(is_warm_enough(&DataTier::Hot, &DataTier::Archive));
        assert!(is_warm_enough(&DataTier::Warm, &DataTier::Warm));
        assert!(!is_warm_enough(&DataTier::Archive, &DataTier::Hot));
        assert!(!is_warm_enough(&DataTier::Cold, &DataTier::Warm));
    }

    // --- Governance Override Tests ---

    #[test]
    fn test_governance_override_grants_open_access() {
        let mut mgr = PublicInterestAccessManager::new();
        // Register a restricted policy
        mgr.register_policy(internal_only_policy("weather.restricted"));

        // Before override: Untrusted user is denied
        let result_before = mgr.check_access(
            "weather.restricted",
            &TrustClass::Untrusted,
            0.0,
            false,
            &DataTier::Archive,
        );
        assert!(!result_before.allowed);

        // Set governance override
        mgr.set_governance_override("weather.restricted");

        // After override: Untrusted user is granted access
        let result_after = mgr.check_access(
            "weather.restricted",
            &TrustClass::Untrusted,
            0.0,
            false,
            &DataTier::Archive,
        );
        assert!(result_after.allowed);
        assert_eq!(result_after.reason, "governance override active");
        assert_eq!(result_after.effective_tier, AccessTier::OpenAccess);
    }

    #[test]
    fn test_governance_override_supersedes_all_constraints() {
        let mut mgr = PublicInterestAccessManager::new();
        // Register a policy with high quality requirement and provenance requirement
        let policy = PublicInterestAccessPolicy {
            report_class: "weather.emergency".to_string(),
            access_tier: AccessTier::InternalOnly,
            min_quality_score: 0.9,
            requires_provenance: true,
            max_storage_tier: DataTier::Hot,
        };
        mgr.register_policy(policy);

        // Set governance override
        mgr.set_governance_override("weather.emergency");

        // All constraints are bypassed: low quality, no provenance, cold tier, untrusted user
        let result = mgr.check_access(
            "weather.emergency",
            &TrustClass::Untrusted,
            0.0,             // fails quality check
            false,           // fails provenance check
            &DataTier::Cold, // fails tier check
        );
        assert!(result.allowed);
        assert_eq!(result.reason, "governance override active");
    }

    #[test]
    fn test_clear_governance_override() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(internal_only_policy("weather.sensitive"));
        mgr.set_governance_override("weather.sensitive");

        assert!(mgr.has_governance_override("weather.sensitive"));
        assert!(mgr.clear_governance_override("weather.sensitive"));
        assert!(!mgr.has_governance_override("weather.sensitive"));

        // After clearing, policy is enforced again
        let result = mgr.check_access(
            "weather.sensitive",
            &TrustClass::Untrusted,
            0.0,
            false,
            &DataTier::Archive,
        );
        assert!(!result.allowed);
    }

    #[test]
    fn test_clear_nonexistent_governance_override() {
        let mut mgr = PublicInterestAccessManager::new();
        // Clearing a non-existent override returns false
        assert!(!mgr.clear_governance_override("nonexistent"));
    }

    #[test]
    fn test_list_governance_overrides() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.set_governance_override("weather.emergency");
        mgr.set_governance_override("air_quality.critical");
        mgr.set_governance_override("flood.warning");

        let overrides = mgr.list_governance_overrides();
        assert_eq!(overrides.len(), 3);
        // Verify sorted order
        assert_eq!(overrides[0], "air_quality.critical");
        assert_eq!(overrides[1], "flood.warning");
        assert_eq!(overrides[2], "weather.emergency");
    }

    #[test]
    fn test_governance_override_is_temporary() {
        let mut mgr = PublicInterestAccessManager::new();
        mgr.register_policy(internal_only_policy("report.classified"));

        // Override temporarily grants access
        mgr.set_governance_override("report.classified");
        let result1 = mgr.check_access(
            "report.classified",
            &TrustClass::Untrusted,
            0.0,
            false,
            &DataTier::Archive,
        );
        assert!(result1.allowed);

        // Clear override
        mgr.clear_governance_override("report.classified");

        // Access is revoked
        let result2 = mgr.check_access(
            "report.classified",
            &TrustClass::Untrusted,
            0.0,
            false,
            &DataTier::Archive,
        );
        assert!(!result2.allowed);
    }
}
