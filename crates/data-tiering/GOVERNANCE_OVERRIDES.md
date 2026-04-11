# Governance Override Hooks

## Overview

The `PublicInterestAccessManager` provides governance override hooks that allow governance bodies to temporarily enforce `OpenAccess` for specific report classes, superseding all standard access control policies.

This mechanism enables rapid response to emergency situations, regulatory mandates, or other governance decisions that require immediate public access to environmental data without requiring code changes or redeployment.

## Use Cases

- **Emergency Response:** Public health officials mandate immediate, unauthenticated access to air quality or flood data during a crisis
- **Regulatory Compliance:** Data protection regulations require verified public-interest reports to be openly accessible
- **Transparency Initiatives:** Governance decides to publish sensitive environmental datasets for public benefit
- **Temporary Access Grants:** Short-term provision of access pending formal policy changes

## Semantics

When a governance override is **active** for a report class:
- ✅ All requesters (including unauthenticated users) gain immediate access
- ✅ All constraints are bypassed:
  - Trust class requirements (Untrusted users are permitted)
  - Quality score thresholds
  - Provenance attestation requirements
  - Storage tier requirements
- ✅ The access check returns `allowed: true` with reason `"governance override active"`
- ✅ The effective tier is reported as `AccessTier::OpenAccess`

## Precedence

Governance overrides have **highest precedence** in the access control chain:

```
Governance Override (if active) ← HIGHEST
    ↓ (if no override)
Registered Policy (if present)
    ↓ (if no registered policy)
Default Tier (RegisteredAccess)
```

## API

### Setting an Override

```rust
manager.set_governance_override("weather.emergency");
```

Grants `OpenAccess` to `weather.emergency` and all matching report classes.

**Parameters:**
- `report_class: &str` — The report class to override (e.g., `"weather.emergency"`, `"air_quality.critical"`)

### Clearing an Override

```rust
let was_active = manager.clear_governance_override("weather.emergency");
```

Removes the override, reverting to standard policy enforcement.

**Returns:** `true` if override was present and removed, `false` otherwise.

### Checking Override Status

```rust
if manager.has_governance_override("weather.emergency") {
    // Override is active
}

let overrides = manager.list_governance_overrides();
// Returns: ["air_quality.critical", "flood.warning", "weather.emergency"]
```

## Example Workflow

```rust
use wrfcoin_data_tiering::PublicInterestAccessManager;
use wrfcoin_storage_common::DataTier;
use wrfcoin_data_tiering::TrustClass;

let mut manager = PublicInterestAccessManager::new();

// Register a restrictive policy for sensitive weather data
let policy = PublicInterestAccessPolicy {
    report_class: "weather.sensitive".to_string(),
    access_tier: AccessTier::InternalOnly, // only verified users
    min_quality_score: 0.8,
    requires_provenance: true,
    max_storage_tier: DataTier::Warm,
};
manager.register_policy(policy);

// Standard access check: untrusted user is denied
let result = manager.check_access(
    "weather.sensitive",
    &TrustClass::Untrusted,
    0.5,
    false,
    &DataTier::Cold,
);
assert!(!result.allowed); // Denied

// Governance decision: public emergency requires immediate transparency
manager.set_governance_override("weather.sensitive");

// Now the same user gains access
let result = manager.check_access(
    "weather.sensitive",
    &TrustClass::Untrusted,
    0.5,
    false,
    &DataTier::Cold,
);
assert!(result.allowed); // Granted via override

// When crisis is resolved, clear the override
manager.clear_governance_override("weather.sensitive");

// Access reverts to standard policy
let result = manager.check_access(
    "weather.sensitive",
    &TrustClass::Untrusted,
    0.5,
    false,
    &DataTier::Cold,
);
assert!(!result.allowed); // Denied again
```

## Audit & Compliance

Governance overrides should be:

1. **Logged** — Every override set/clear should emit an audit log with:
   - Timestamp
   - Report class
   - Action (set/clear)
   - Authorized personnel
   - Reason (e.g., "emergency response", "regulatory mandate")

2. **Temporary** — Overrides are not persistent across restarts. They should be explicitly set at startup or via administrative API if persistence is required.

3. **Observable** — Use `list_governance_overrides()` to generate periodic compliance reports.

Example audit log format:
```json
{
  "timestamp": "2026-04-09T14:32:00Z",
  "action": "governance_override_set",
  "report_class": "weather.emergency",
  "reason": "hurricane evacuation order — mandatory public alert",
  "authorized_by": "emergency_management_dept",
  "expected_duration_secs": 86400
}
```

## Integration with Data Tiering

Governance overrides integrate seamlessly with the data-tiering system:

- **Storage Tier:** Overridden content can still be stored in any tier; the override only affects read access.
- **Quality Scoring:** Quality assessments continue independently of overrides.
- **Migration Policies:** Tier migrations proceed normally; overrides only affect access control.

## Non-Goals

Governance overrides **do not**:
- Modify underlying policies (only bypass them temporarily)
- Persist across process restarts (in-memory only)
- Provide fine-grained access control (all-or-nothing by report class)
- Replace formal policy changes (use for emergency response, not permanent policy)

## Testing

The `PublicInterestAccessManager` includes comprehensive tests:
- `test_governance_override_grants_open_access` — Override grants access to restricted content
- `test_governance_override_supersedes_all_constraints` — All constraints are bypassed
- `test_clear_governance_override` — Clearing reverts to standard policy
- `test_governance_override_is_temporary` — Override is not persistent
- `test_list_governance_overrides` — Enumerate active overrides

Run tests:
```bash
cargo test -p wrfcoin-data-tiering --lib public_access
```
