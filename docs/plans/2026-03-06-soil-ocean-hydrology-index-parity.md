# Soil/Ocean/Hydrology Index Parity Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Achieve full index parity for soil, ocean, and hydrology payload families — domain-specific validation, a queryable content index by domain, default retention policies, and end-to-end integration tests proving consistent ingestion/retrieval across all providers.

**Architecture:** The payload types and enum variants already exist in `storage-common/src/types.rs`. This plan adds three layers on top: (1) physical-bounds validation on each payload, (2) a `DomainContentIndex` that the orchestrator populates on store and exposes for domain-level queries, and (3) default retention policies per domain. All changes stay within the existing crate boundaries.

**Tech Stack:** Rust 2021, serde, tokio, async-trait (workspace deps already present)

**Issue:** storage-providers#6
**Dependencies:** Backend normalization pipelines for soil/ocean/hydrology — verified as satisfied (NRCS soil service, USGS water service, environment-phase2-reports-service all exist in `backend/`).

---

## Task 1: Add physical-bounds validation to SoilPayload

**Files:**
- Modify: `crates/storage-common/src/types.rs` (add `validate()` method to `SoilPayload`)
- Test: same file, `mod tests`

**Step 1: Write the failing tests**

Add to the `mod tests` block in `types.rs`, after the existing soil tests:

```rust
// ---- Soil payload validation tests ----

#[test]
fn test_soil_payload_validate_valid() {
    let p = SoilPayload {
        moisture_pct: Some(32.5),
        temperature_c: Some(18.0),
        ph: Some(6.8),
        organic_carbon_pct: Some(2.1),
        nitrogen_mgl: Some(15.0),
        phosphorus_mgl: Some(8.5),
        potassium_mgl: Some(120.0),
        conductivity_dsm: Some(0.45),
        depth_cm: Some(10.0),
    };
    assert!(p.validate().is_ok());
}

#[test]
fn test_soil_payload_validate_all_none() {
    let p = SoilPayload {
        moisture_pct: None, temperature_c: None, ph: None,
        organic_carbon_pct: None, nitrogen_mgl: None, phosphorus_mgl: None,
        potassium_mgl: None, conductivity_dsm: None, depth_cm: None,
    };
    assert!(p.validate().is_ok());
}

#[test]
fn test_soil_payload_validate_ph_out_of_range() {
    let p = SoilPayload {
        ph: Some(15.0), moisture_pct: None, temperature_c: None,
        organic_carbon_pct: None, nitrogen_mgl: None, phosphorus_mgl: None,
        potassium_mgl: None, conductivity_dsm: None, depth_cm: None,
    };
    let err = p.validate().unwrap_err();
    assert!(err.contains("ph"));
}

#[test]
fn test_soil_payload_validate_moisture_out_of_range() {
    let p = SoilPayload {
        moisture_pct: Some(150.0), ph: None, temperature_c: None,
        organic_carbon_pct: None, nitrogen_mgl: None, phosphorus_mgl: None,
        potassium_mgl: None, conductivity_dsm: None, depth_cm: None,
    };
    let err = p.validate().unwrap_err();
    assert!(err.contains("moisture"));
}

#[test]
fn test_soil_payload_validate_negative_depth() {
    let p = SoilPayload {
        depth_cm: Some(-5.0), moisture_pct: None, temperature_c: None, ph: None,
        organic_carbon_pct: None, nitrogen_mgl: None, phosphorus_mgl: None,
        potassium_mgl: None, conductivity_dsm: None,
    };
    let err = p.validate().unwrap_err();
    assert!(err.contains("depth"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p wrfcoin-storage-common -- soil_payload_validate`
Expected: FAIL — `validate` method not found on `SoilPayload`

**Step 3: Implement SoilPayload::validate()**

Add this `impl` block in `types.rs` after the `SoilPayload` struct definition (around line 237):

```rust
impl SoilPayload {
    /// Validate physical bounds of soil measurements.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(v) = self.moisture_pct {
            if !(0.0..=100.0).contains(&v) {
                return Err(format!("moisture_pct {v} out of range [0, 100]"));
            }
        }
        if let Some(v) = self.ph {
            if !(0.0..=14.0).contains(&v) {
                return Err(format!("ph {v} out of range [0, 14]"));
            }
        }
        if let Some(v) = self.organic_carbon_pct {
            if !(0.0..=100.0).contains(&v) {
                return Err(format!("organic_carbon_pct {v} out of range [0, 100]"));
            }
        }
        if let Some(v) = self.depth_cm {
            if v < 0.0 {
                return Err(format!("depth_cm {v} must be non-negative"));
            }
        }
        if let Some(v) = self.nitrogen_mgl {
            if v < 0.0 {
                return Err(format!("nitrogen_mgl {v} must be non-negative"));
            }
        }
        if let Some(v) = self.phosphorus_mgl {
            if v < 0.0 {
                return Err(format!("phosphorus_mgl {v} must be non-negative"));
            }
        }
        if let Some(v) = self.potassium_mgl {
            if v < 0.0 {
                return Err(format!("potassium_mgl {v} must be non-negative"));
            }
        }
        if let Some(v) = self.conductivity_dsm {
            if v < 0.0 {
                return Err(format!("conductivity_dsm {v} must be non-negative"));
            }
        }
        Ok(())
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wrfcoin-storage-common -- soil_payload_validate`
Expected: PASS (5 tests)

**Step 5: Commit**

```bash
git add crates/storage-common/src/types.rs
git commit -m "feat: add physical-bounds validation for SoilPayload (#6)"
```

---

## Task 2: Add physical-bounds validation to OceanPayload

**Files:**
- Modify: `crates/storage-common/src/types.rs`
- Test: same file, `mod tests`

**Step 1: Write the failing tests**

```rust
// ---- Ocean payload validation tests ----

#[test]
fn test_ocean_payload_validate_valid() {
    let p = OceanPayload {
        sea_surface_temperature_c: Some(28.3),
        salinity_psu: Some(35.2),
        wave_height_m: Some(1.8),
        wave_period_s: Some(9.5),
        current_speed_ms: Some(0.4),
        current_direction_deg: Some(180),
        chlorophyll_a_mgl: Some(0.25),
        dissolved_oxygen_mgl: Some(7.1),
        depth_m: Some(0.5),
    };
    assert!(p.validate().is_ok());
}

#[test]
fn test_ocean_payload_validate_all_none() {
    let p = OceanPayload {
        sea_surface_temperature_c: None, salinity_psu: None,
        wave_height_m: None, wave_period_s: None, current_speed_ms: None,
        current_direction_deg: None, chlorophyll_a_mgl: None,
        dissolved_oxygen_mgl: None, depth_m: None,
    };
    assert!(p.validate().is_ok());
}

#[test]
fn test_ocean_payload_validate_salinity_out_of_range() {
    let p = OceanPayload {
        salinity_psu: Some(60.0),
        sea_surface_temperature_c: None, wave_height_m: None,
        wave_period_s: None, current_speed_ms: None,
        current_direction_deg: None, chlorophyll_a_mgl: None,
        dissolved_oxygen_mgl: None, depth_m: None,
    };
    let err = p.validate().unwrap_err();
    assert!(err.contains("salinity"));
}

#[test]
fn test_ocean_payload_validate_negative_wave_height() {
    let p = OceanPayload {
        wave_height_m: Some(-1.0),
        sea_surface_temperature_c: None, salinity_psu: None,
        wave_period_s: None, current_speed_ms: None,
        current_direction_deg: None, chlorophyll_a_mgl: None,
        dissolved_oxygen_mgl: None, depth_m: None,
    };
    let err = p.validate().unwrap_err();
    assert!(err.contains("wave_height"));
}

#[test]
fn test_ocean_payload_validate_direction_over_360() {
    let p = OceanPayload {
        current_direction_deg: Some(361),
        sea_surface_temperature_c: None, salinity_psu: None,
        wave_height_m: None, wave_period_s: None, current_speed_ms: None,
        chlorophyll_a_mgl: None, dissolved_oxygen_mgl: None, depth_m: None,
    };
    let err = p.validate().unwrap_err();
    assert!(err.contains("current_direction"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p wrfcoin-storage-common -- ocean_payload_validate`
Expected: FAIL — `validate` method not found on `OceanPayload`

**Step 3: Implement OceanPayload::validate()**

Add after the `OceanPayload` struct definition (around line 251):

```rust
impl OceanPayload {
    /// Validate physical bounds of ocean measurements.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(v) = self.salinity_psu {
            if !(0.0..=50.0).contains(&v) {
                return Err(format!("salinity_psu {v} out of range [0, 50]"));
            }
        }
        if let Some(v) = self.wave_height_m {
            if v < 0.0 {
                return Err(format!("wave_height_m {v} must be non-negative"));
            }
        }
        if let Some(v) = self.wave_period_s {
            if v < 0.0 {
                return Err(format!("wave_period_s {v} must be non-negative"));
            }
        }
        if let Some(v) = self.current_speed_ms {
            if v < 0.0 {
                return Err(format!("current_speed_ms {v} must be non-negative"));
            }
        }
        if let Some(dir) = self.current_direction_deg {
            if dir > 360 {
                return Err(format!("current_direction_deg {dir} out of range [0, 360]"));
            }
        }
        if let Some(v) = self.chlorophyll_a_mgl {
            if v < 0.0 {
                return Err(format!("chlorophyll_a_mgl {v} must be non-negative"));
            }
        }
        if let Some(v) = self.dissolved_oxygen_mgl {
            if v < 0.0 {
                return Err(format!("dissolved_oxygen_mgl {v} must be non-negative"));
            }
        }
        if let Some(v) = self.depth_m {
            if v < 0.0 {
                return Err(format!("depth_m {v} must be non-negative"));
            }
        }
        Ok(())
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wrfcoin-storage-common -- ocean_payload_validate`
Expected: PASS (5 tests)

**Step 5: Commit**

```bash
git add crates/storage-common/src/types.rs
git commit -m "feat: add physical-bounds validation for OceanPayload (#6)"
```

---

## Task 3: Add physical-bounds validation to HydrologyPayload

**Files:**
- Modify: `crates/storage-common/src/types.rs`
- Test: same file, `mod tests`

**Step 1: Write the failing tests**

```rust
// ---- Hydrology payload validation tests ----

#[test]
fn test_hydrology_payload_validate_valid() {
    let p = HydrologyPayload {
        river_discharge_m3s: Some(1250.0),
        water_level_m: Some(4.3),
        flow_velocity_ms: Some(0.85),
        sediment_load_mgl: Some(320.0),
        groundwater_level_m: Some(2.1),
        reservoir_storage_pct: Some(72.0),
        flood_risk_level: Some(2),
        drought_index: Some(-0.5),
    };
    assert!(p.validate().is_ok());
}

#[test]
fn test_hydrology_payload_validate_all_none() {
    let p = HydrologyPayload {
        river_discharge_m3s: None, water_level_m: None,
        flow_velocity_ms: None, sediment_load_mgl: None,
        groundwater_level_m: None, reservoir_storage_pct: None,
        flood_risk_level: None, drought_index: None,
    };
    assert!(p.validate().is_ok());
}

#[test]
fn test_hydrology_payload_validate_reservoir_over_100() {
    let p = HydrologyPayload {
        reservoir_storage_pct: Some(110.0),
        river_discharge_m3s: None, water_level_m: None,
        flow_velocity_ms: None, sediment_load_mgl: None,
        groundwater_level_m: None, flood_risk_level: None,
        drought_index: None,
    };
    let err = p.validate().unwrap_err();
    assert!(err.contains("reservoir_storage"));
}

#[test]
fn test_hydrology_payload_validate_flood_risk_over_5() {
    let p = HydrologyPayload {
        flood_risk_level: Some(6),
        river_discharge_m3s: None, water_level_m: None,
        flow_velocity_ms: None, sediment_load_mgl: None,
        groundwater_level_m: None, reservoir_storage_pct: None,
        drought_index: None,
    };
    let err = p.validate().unwrap_err();
    assert!(err.contains("flood_risk"));
}

#[test]
fn test_hydrology_payload_validate_negative_discharge() {
    let p = HydrologyPayload {
        river_discharge_m3s: Some(-10.0),
        water_level_m: None, flow_velocity_ms: None,
        sediment_load_mgl: None, groundwater_level_m: None,
        reservoir_storage_pct: None, flood_risk_level: None,
        drought_index: None,
    };
    let err = p.validate().unwrap_err();
    assert!(err.contains("river_discharge"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p wrfcoin-storage-common -- hydrology_payload_validate`
Expected: FAIL — `validate` method not found on `HydrologyPayload`

**Step 3: Implement HydrologyPayload::validate()**

Add after the `HydrologyPayload` struct definition (around line 264):

```rust
impl HydrologyPayload {
    /// Validate physical bounds of hydrology measurements.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(v) = self.river_discharge_m3s {
            if v < 0.0 {
                return Err(format!("river_discharge_m3s {v} must be non-negative"));
            }
        }
        if let Some(v) = self.flow_velocity_ms {
            if v < 0.0 {
                return Err(format!("flow_velocity_ms {v} must be non-negative"));
            }
        }
        if let Some(v) = self.sediment_load_mgl {
            if v < 0.0 {
                return Err(format!("sediment_load_mgl {v} must be non-negative"));
            }
        }
        if let Some(v) = self.reservoir_storage_pct {
            if !(0.0..=100.0).contains(&v) {
                return Err(format!("reservoir_storage_pct {v} out of range [0, 100]"));
            }
        }
        if let Some(v) = self.flood_risk_level {
            if v > 5 {
                return Err(format!("flood_risk_level {v} out of range [0, 5]"));
            }
        }
        Ok(())
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wrfcoin-storage-common -- hydrology_payload_validate`
Expected: PASS (5 tests)

**Step 5: Commit**

```bash
git add crates/storage-common/src/types.rs
git commit -m "feat: add physical-bounds validation for HydrologyPayload (#6)"
```

---

## Task 4: Wire payload validation into EnvironmentalReport::validate()

**Files:**
- Modify: `crates/storage-common/src/types.rs` (update `EnvironmentalReport::validate()`)
- Test: same file, `mod tests`

**Step 1: Write the failing test**

```rust
#[test]
fn test_report_validate_rejects_invalid_soil_payload() {
    let mut report = sample_soil_report();
    // Set pH to invalid value
    if let ReportPayload::Soil(ref mut p) = report.payload {
        p.ph = Some(20.0);
    }
    let err = report.validate().unwrap_err();
    assert!(err.contains("ph"));
}

#[test]
fn test_report_validate_rejects_invalid_ocean_payload() {
    let mut report = sample_ocean_report();
    if let ReportPayload::Ocean(ref mut p) = report.payload {
        p.salinity_psu = Some(-5.0);
    }
    let err = report.validate().unwrap_err();
    assert!(err.contains("salinity"));
}

#[test]
fn test_report_validate_rejects_invalid_hydrology_payload() {
    let mut report = sample_hydrology_report();
    if let ReportPayload::Hydrology(ref mut p) = report.payload {
        p.reservoir_storage_pct = Some(200.0);
    }
    let err = report.validate().unwrap_err();
    assert!(err.contains("reservoir_storage"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p wrfcoin-storage-common -- report_validate_rejects_invalid`
Expected: FAIL — current `validate()` doesn't check payload bounds

**Step 3: Add payload validation call to EnvironmentalReport::validate()**

Modify the existing `validate()` method in `EnvironmentalReport` (around line 321). After the `quality_score` check and before the final `Ok(())`, add:

```rust
        // Validate domain-specific payload bounds.
        match &self.payload {
            ReportPayload::Soil(p) => p.validate()?,
            ReportPayload::Ocean(p) => p.validate()?,
            ReportPayload::Hydrology(p) => p.validate()?,
            _ => {} // Other domains: no bounds validation yet
        }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wrfcoin-storage-common -- report_validate_rejects_invalid`
Expected: PASS (3 tests)

**Step 5: Run full crate tests**

Run: `cargo test -p wrfcoin-storage-common`
Expected: All tests pass (existing + new)

**Step 6: Commit**

```bash
git add crates/storage-common/src/types.rs
git commit -m "feat: wire payload validation into EnvironmentalReport::validate (#6)"
```

---

## Task 5: Add DomainContentIndex to storage-common

**Files:**
- Create: `crates/storage-common/src/domain_index.rs`
- Modify: `crates/storage-common/src/lib.rs` (add module + re-export)

**Step 1: Write the failing tests (inline in domain_index.rs)**

```rust
// crates/storage-common/src/domain_index.rs

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
///
/// Enables O(1) domain-level queries (e.g., "list all soil reports") and
/// O(1) content_id lookups for migration status.
pub struct DomainContentIndex {
    by_domain: HashMap<EnvironmentalDomain, Vec<IndexEntry>>,
    by_content_id: HashMap<String, IndexEntry>,
}

impl DomainContentIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self {
            by_domain: HashMap::new(),
            by_content_id: HashMap::new(),
        }
    }

    /// Insert an entry into the index.
    pub fn insert(&mut self, entry: IndexEntry) {
        let key = entry.content_id.to_string();
        self.by_domain
            .entry(entry.domain.clone())
            .or_default()
            .push(entry.clone());
        self.by_content_id.insert(key, entry);
    }

    /// Query all entries for a given domain.
    pub fn query_by_domain(&self, domain: &EnvironmentalDomain) -> &[IndexEntry] {
        self.by_domain
            .get(domain)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Look up a single entry by content ID string.
    pub fn get_by_content_id(&self, content_id_str: &str) -> Option<&IndexEntry> {
        self.by_content_id.get(content_id_str)
    }

    /// Return the count of entries for a given domain.
    pub fn count_by_domain(&self, domain: &EnvironmentalDomain) -> usize {
        self.by_domain
            .get(domain)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Return total indexed entries.
    pub fn total_count(&self) -> usize {
        self.by_content_id.len()
    }

    /// Remove an entry by content ID string. Returns true if found and removed.
    pub fn remove(&mut self, content_id_str: &str) -> bool {
        let Some(entry) = self.by_content_id.remove(content_id_str) else {
            return false;
        };
        if let Some(entries) = self.by_domain.get_mut(&entry.domain) {
            entries.retain(|e| e.content_id.to_string() != content_id_str);
        }
        true
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
            content_id: ContentId { provider: "ipfs".into(), hash: hash.into() },
            domain: EnvironmentalDomain::Soil,
            station_id: "soil-stn-1".into(),
            report_id: format!("soil-{hash}"),
            stored_at: 1000,
        }
    }

    fn ocean_entry(hash: &str) -> IndexEntry {
        IndexEntry {
            content_id: ContentId { provider: "ipfs".into(), hash: hash.into() },
            domain: EnvironmentalDomain::Ocean,
            station_id: "buoy-1".into(),
            report_id: format!("ocean-{hash}"),
            stored_at: 2000,
        }
    }

    #[test]
    fn test_insert_and_query_by_domain() {
        let mut idx = DomainContentIndex::new();
        idx.insert(soil_entry("a"));
        idx.insert(soil_entry("b"));
        idx.insert(ocean_entry("c"));
        assert_eq!(idx.query_by_domain(&EnvironmentalDomain::Soil).len(), 2);
        assert_eq!(idx.query_by_domain(&EnvironmentalDomain::Ocean).len(), 1);
        assert_eq!(idx.query_by_domain(&EnvironmentalDomain::Hydrology).len(), 0);
    }

    #[test]
    fn test_get_by_content_id() {
        let mut idx = DomainContentIndex::new();
        idx.insert(soil_entry("abc"));
        let entry = idx.get_by_content_id("ipfs:abc").unwrap();
        assert_eq!(entry.station_id, "soil-stn-1");
    }

    #[test]
    fn test_count_by_domain() {
        let mut idx = DomainContentIndex::new();
        idx.insert(soil_entry("x"));
        idx.insert(soil_entry("y"));
        assert_eq!(idx.count_by_domain(&EnvironmentalDomain::Soil), 2);
        assert_eq!(idx.count_by_domain(&EnvironmentalDomain::Ocean), 0);
    }

    #[test]
    fn test_total_count() {
        let mut idx = DomainContentIndex::new();
        idx.insert(soil_entry("a"));
        idx.insert(ocean_entry("b"));
        assert_eq!(idx.total_count(), 2);
    }

    #[test]
    fn test_remove() {
        let mut idx = DomainContentIndex::new();
        idx.insert(soil_entry("rm"));
        assert!(idx.remove("ipfs:rm"));
        assert_eq!(idx.total_count(), 0);
        assert_eq!(idx.count_by_domain(&EnvironmentalDomain::Soil), 0);
        assert!(!idx.remove("ipfs:rm")); // already removed
    }

    #[test]
    fn test_query_empty_domain() {
        let idx = DomainContentIndex::new();
        assert!(idx.query_by_domain(&EnvironmentalDomain::Wildfire).is_empty());
    }
}
```

**Step 2: Add module to lib.rs**

In `crates/storage-common/src/lib.rs`, add:

```rust
pub mod domain_index;
```

And add to the `pub use` block:

```rust
pub use domain_index::{DomainContentIndex, IndexEntry};
```

**Step 3: Run tests to verify they pass**

Run: `cargo test -p wrfcoin-storage-common -- domain_index`
Expected: PASS (6 tests)

**Step 4: Commit**

```bash
git add crates/storage-common/src/domain_index.rs crates/storage-common/src/lib.rs
git commit -m "feat: add DomainContentIndex for domain-level content queries (#6)"
```

---

## Task 6: Wire DomainContentIndex into DataTieringOrchestrator

**Files:**
- Modify: `crates/data-tiering/src/orchestrator.rs`
- Test: same file, `mod tests`

**Step 1: Write the failing tests**

Add to the `mod tests` block:

```rust
#[tokio::test]
async fn test_store_environmental_report_indexes_by_domain() {
    use wrfcoin_storage_common::{
        EnvironmentalDomain, EnvironmentalReport, GeoLocation, ReportPayload,
        SoilPayload, OceanPayload, HydrologyPayload,
    };

    let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
    orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

    // Relax provenance requirement for this test.
    orch.set_provenance_policy(ProvenancePolicy {
        require_provenance: false,
        min_chain_length: 0,
        allowed_algorithms: Vec::new(),
    }).unwrap();

    let soil_report = EnvironmentalReport {
        report_id: "soil-idx-1".into(),
        domain: EnvironmentalDomain::Soil,
        station_id: "stn-s1".into(),
        location: GeoLocation { latitude: 45.0, longitude: -93.0, altitude_m: None },
        observed_at: 1000, submitted_at: 1001,
        payload: ReportPayload::Soil(SoilPayload {
            moisture_pct: Some(30.0), temperature_c: None, ph: Some(6.5),
            organic_carbon_pct: None, nitrogen_mgl: None, phosphorus_mgl: None,
            potassium_mgl: None, conductivity_dsm: None, depth_cm: Some(10.0),
        }),
        quality_score: Some(0.9),
        provenance: None,
    };

    orch.store_environmental_report(&soil_report, DataTier::Hot).await.unwrap();

    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Soil).len(), 1);
    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Ocean).len(), 0);
    assert_eq!(orch.domain_index_total(), 1);
}

#[tokio::test]
async fn test_domain_index_multiple_domains() {
    use wrfcoin_storage_common::{
        EnvironmentalDomain, EnvironmentalReport, GeoLocation, ReportPayload,
        SoilPayload, OceanPayload, HydrologyPayload,
    };

    let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
    orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
    orch.set_provenance_policy(ProvenancePolicy {
        require_provenance: false,
        min_chain_length: 0,
        allowed_algorithms: Vec::new(),
    }).unwrap();

    let make_report = |id: &str, domain: EnvironmentalDomain, payload: ReportPayload| {
        EnvironmentalReport {
            report_id: id.into(),
            domain,
            station_id: "stn-1".into(),
            location: GeoLocation { latitude: 0.0, longitude: 0.0, altitude_m: None },
            observed_at: 1000, submitted_at: 1001,
            payload,
            quality_score: None,
            provenance: None,
        }
    };

    let soil = make_report("s1", EnvironmentalDomain::Soil, ReportPayload::Soil(SoilPayload {
        moisture_pct: Some(20.0), temperature_c: None, ph: None,
        organic_carbon_pct: None, nitrogen_mgl: None, phosphorus_mgl: None,
        potassium_mgl: None, conductivity_dsm: None, depth_cm: None,
    }));
    let ocean = make_report("o1", EnvironmentalDomain::Ocean, ReportPayload::Ocean(OceanPayload {
        sea_surface_temperature_c: Some(25.0), salinity_psu: None,
        wave_height_m: None, wave_period_s: None, current_speed_ms: None,
        current_direction_deg: None, chlorophyll_a_mgl: None,
        dissolved_oxygen_mgl: None, depth_m: None,
    }));
    let hydro = make_report("h1", EnvironmentalDomain::Hydrology, ReportPayload::Hydrology(HydrologyPayload {
        river_discharge_m3s: Some(500.0), water_level_m: None,
        flow_velocity_ms: None, sediment_load_mgl: None,
        groundwater_level_m: None, reservoir_storage_pct: None,
        flood_risk_level: None, drought_index: None,
    }));

    orch.store_environmental_report(&soil, DataTier::Hot).await.unwrap();
    orch.store_environmental_report(&ocean, DataTier::Hot).await.unwrap();
    orch.store_environmental_report(&hydro, DataTier::Hot).await.unwrap();

    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Soil).len(), 1);
    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Ocean).len(), 1);
    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Hydrology).len(), 1);
    assert_eq!(orch.domain_index_total(), 3);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p wrfcoin-data-tiering -- domain_index`
Expected: FAIL — `query_domain_index` and `domain_index_total` methods don't exist

**Step 3: Implement domain index in orchestrator**

In `orchestrator.rs`:

1. Add import at the top:
```rust
use wrfcoin_storage_common::domain_index::{DomainContentIndex, IndexEntry};
use wrfcoin_storage_common::EnvironmentalDomain;
```

2. Add field to `DataTieringOrchestrator`:
```rust
    domain_index: Arc<RwLock<DomainContentIndex>>,
```

3. Initialize in `new()`:
```rust
    domain_index: Arc::new(RwLock::new(DomainContentIndex::new())),
```

4. In `store_environmental_report`, after the `store_with_provenance_enforcement` call returns `Accepted`, insert into the domain index. Replace the existing `store_with_provenance_enforcement` call with:
```rust
        let decision = self.store_with_provenance_enforcement(&bytes, &metadata, &tier, provenance)
            .await?;

        if decision == AdmissionDecision::Accepted {
            if let Ok(mut idx) = self.domain_index.write() {
                idx.insert(IndexEntry {
                    content_id: ContentId {
                        provider: format!("{tier:?}"),
                        hash: metadata.tags.get("report_id").cloned().unwrap_or_default(),
                    },
                    domain: report.domain.clone(),
                    station_id: report.station_id.clone(),
                    report_id: report.report_id.clone(),
                    stored_at: report.submitted_at,
                });
            }
        }

        Ok(decision)
```

5. Add public query methods:
```rust
    /// Query the domain content index for a given environmental domain.
    pub fn query_domain_index(&self, domain: &EnvironmentalDomain) -> Vec<IndexEntry> {
        self.domain_index
            .read()
            .map(|idx| idx.query_by_domain(domain).to_vec())
            .unwrap_or_default()
    }

    /// Return total entries in the domain content index.
    pub fn domain_index_total(&self) -> usize {
        self.domain_index
            .read()
            .map(|idx| idx.total_count())
            .unwrap_or(0)
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wrfcoin-data-tiering -- domain_index`
Expected: PASS (2 tests)

**Step 5: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass

**Step 6: Commit**

```bash
git add crates/data-tiering/src/orchestrator.rs
git commit -m "feat: wire DomainContentIndex into orchestrator for domain queries (#6)"
```

---

## Task 7: Add default retention policies for soil/ocean/hydrology

**Files:**
- Modify: `crates/data-tiering/src/retention.rs` (add `with_environmental_defaults()`)
- Test: same file, `mod tests`

**Step 1: Write the failing tests**

```rust
#[test]
fn test_environmental_defaults_include_soil() {
    let mgr = RetentionManager::with_environmental_defaults();
    let policies = mgr.policies_for("soil.observation");
    assert!(!policies.is_empty());
    assert!(policies[0].min_copies >= 2);
}

#[test]
fn test_environmental_defaults_include_ocean() {
    let mgr = RetentionManager::with_environmental_defaults();
    let policies = mgr.policies_for("ocean.observation");
    assert!(!policies.is_empty());
}

#[test]
fn test_environmental_defaults_include_hydrology() {
    let mgr = RetentionManager::with_environmental_defaults();
    let policies = mgr.policies_for("hydrology.observation");
    assert!(!policies.is_empty());
}

#[test]
fn test_environmental_defaults_soil_long_retention() {
    let mgr = RetentionManager::with_environmental_defaults();
    let policies = mgr.policies_for("soil.observation");
    // Soil data is valuable long-term — max_age_secs should be >= 1 year
    assert!(policies.iter().any(|p| p.max_age_secs == 0 || p.max_age_secs >= 365 * 86400));
}

#[test]
fn test_environmental_defaults_hydrology_min_copies() {
    let mgr = RetentionManager::with_environmental_defaults();
    // Hydrology (flood risk) data needs redundancy
    assert!(mgr.min_copies_for("hydrology.observation") >= 3);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p wrfcoin-data-tiering -- environmental_defaults`
Expected: FAIL — `with_environmental_defaults` not found

**Step 3: Implement with_environmental_defaults()**

Add to `RetentionManager` impl block in `retention.rs`:

```rust
    /// Create a retention manager pre-loaded with default policies for all
    /// environmental observation domains.
    pub fn with_environmental_defaults() -> Self {
        let mut mgr = Self::new();

        // Weather — high volume, moderate retention
        let mut weather = RetentionPolicy::new("weather.observation", DataTier::Cold, 2);
        weather.max_age_secs = 180 * 86400; // 180 days
        mgr.add_policy(weather);

        // Air quality — regulatory value, longer retention
        let mut aq = RetentionPolicy::new("air_quality.observation", DataTier::Cold, 2);
        aq.max_age_secs = 365 * 86400; // 1 year
        mgr.add_policy(aq);

        // Water quality — environmental monitoring
        let mut wq = RetentionPolicy::new("water_quality.observation", DataTier::Cold, 2);
        wq.max_age_secs = 365 * 86400;
        mgr.add_policy(wq);

        // Wildfire — critical safety data
        let mut wildfire = RetentionPolicy::new("wildfire.observation", DataTier::Cold, 3);
        wildfire.max_age_secs = 365 * 86400;
        mgr.add_policy(wildfire);

        // Soil — long-term agricultural/climate value
        let mut soil = RetentionPolicy::new("soil.observation", DataTier::Cold, 2);
        soil.max_age_secs = 0; // never expires
        mgr.add_policy(soil);
        let mut soil_archive = RetentionPolicy::new("soil.observation", DataTier::Archive, 2);
        soil_archive.max_age_secs = 0;
        mgr.add_policy(soil_archive);

        // Ocean — climate science, long-term
        let mut ocean = RetentionPolicy::new("ocean.observation", DataTier::Cold, 2);
        ocean.max_age_secs = 0; // never expires
        mgr.add_policy(ocean);
        let mut ocean_archive = RetentionPolicy::new("ocean.observation", DataTier::Archive, 2);
        ocean_archive.max_age_secs = 0;
        mgr.add_policy(ocean_archive);

        // Hydrology — flood risk, high redundancy
        let mut hydro = RetentionPolicy::new("hydrology.observation", DataTier::Cold, 3);
        hydro.max_age_secs = 0; // never expires
        mgr.add_policy(hydro);
        let mut hydro_archive = RetentionPolicy::new("hydrology.observation", DataTier::Archive, 3);
        hydro_archive.max_age_secs = 0;
        mgr.add_policy(hydro_archive);

        mgr
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wrfcoin-data-tiering -- environmental_defaults`
Expected: PASS (5 tests)

**Step 5: Commit**

```bash
git add crates/data-tiering/src/retention.rs
git commit -m "feat: add default retention policies for all environmental domains (#6)"
```

---

## Task 8: End-to-end integration tests for soil/ocean/hydrology

**Files:**
- Create: `crates/data-tiering/tests/domain_parity_integration.rs`

**Step 1: Write the integration test file**

```rust
//! Integration tests verifying index parity for soil, ocean, and hydrology
//! payload families across the data-tiering orchestrator.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use wrfcoin_storage_common::{
    AdmissionDecision, ContentId, DataTier, EnvironmentalDomain, EnvironmentalReport,
    GeoLocation, HealthStatus, HydrologyPayload, Metadata, OceanPayload, ReportPayload,
    SoilPayload, StorageError, StorageProvider, StorageStats,
};
use wrfcoin_data_tiering::{DataTieringOrchestrator, ProvenancePolicy, TieringConfig};

/// In-memory mock provider for integration testing.
struct MemoryProvider {
    name: String,
    data: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryProvider {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            data: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl StorageProvider for MemoryProvider {
    async fn store(&self, data: &[u8], _metadata: &Metadata) -> Result<ContentId, StorageError> {
        let hash = format!("{:016x}", {
            let mut h: u64 = 5381;
            for &b in data { h = h.wrapping_mul(33).wrapping_add(b as u64); }
            h
        });
        self.data.lock().unwrap().insert(hash.clone(), data.to_vec());
        Ok(ContentId { provider: self.name.clone(), hash })
    }
    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        self.data.lock().unwrap().get(&id.hash).cloned()
            .ok_or_else(|| StorageError::NotFound(id.to_string()))
    }
    async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
        Ok(self.data.lock().unwrap().contains_key(&id.hash))
    }
    async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
        self.data.lock().unwrap().remove(&id.hash);
        Ok(())
    }
    async fn health_check(&self) -> Result<HealthStatus, StorageError> {
        Ok(HealthStatus { healthy: true, provider: self.name.clone(), latency_ms: Some(1), message: None })
    }
    async fn stats(&self) -> Result<StorageStats, StorageError> {
        Ok(StorageStats::default())
    }
}

fn make_orchestrator() -> DataTieringOrchestrator {
    let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
    orch.register_tier(DataTier::Hot, Box::new(MemoryProvider::new("hot")));
    orch.register_tier(DataTier::Cold, Box::new(MemoryProvider::new("cold")));
    orch.register_tier(DataTier::Archive, Box::new(MemoryProvider::new("archive")));
    orch.set_provenance_policy(ProvenancePolicy {
        require_provenance: false,
        min_chain_length: 0,
        allowed_algorithms: Vec::new(),
    }).unwrap();
    orch
}

fn soil_report(id: &str) -> EnvironmentalReport {
    EnvironmentalReport {
        report_id: id.into(),
        domain: EnvironmentalDomain::Soil,
        station_id: "soil-stn-1".into(),
        location: GeoLocation { latitude: 45.0, longitude: -93.0, altitude_m: None },
        observed_at: 1000, submitted_at: 1001,
        payload: ReportPayload::Soil(SoilPayload {
            moisture_pct: Some(30.0), temperature_c: Some(18.0), ph: Some(6.5),
            organic_carbon_pct: Some(2.0), nitrogen_mgl: Some(10.0),
            phosphorus_mgl: Some(5.0), potassium_mgl: Some(100.0),
            conductivity_dsm: Some(0.4), depth_cm: Some(15.0),
        }),
        quality_score: Some(0.9),
        provenance: None,
    }
}

fn ocean_report(id: &str) -> EnvironmentalReport {
    EnvironmentalReport {
        report_id: id.into(),
        domain: EnvironmentalDomain::Ocean,
        station_id: "buoy-7".into(),
        location: GeoLocation { latitude: 28.5, longitude: -89.2, altitude_m: None },
        observed_at: 2000, submitted_at: 2001,
        payload: ReportPayload::Ocean(OceanPayload {
            sea_surface_temperature_c: Some(28.0), salinity_psu: Some(35.0),
            wave_height_m: Some(1.5), wave_period_s: Some(8.0),
            current_speed_ms: Some(0.3), current_direction_deg: Some(180),
            chlorophyll_a_mgl: Some(0.2), dissolved_oxygen_mgl: Some(7.0),
            depth_m: Some(0.5),
        }),
        quality_score: Some(0.85),
        provenance: None,
    }
}

fn hydrology_report(id: &str) -> EnvironmentalReport {
    EnvironmentalReport {
        report_id: id.into(),
        domain: EnvironmentalDomain::Hydrology,
        station_id: "gauge-1".into(),
        location: GeoLocation { latitude: 44.9, longitude: -93.1, altitude_m: Some(215.0) },
        observed_at: 3000, submitted_at: 3001,
        payload: ReportPayload::Hydrology(HydrologyPayload {
            river_discharge_m3s: Some(800.0), water_level_m: Some(3.5),
            flow_velocity_ms: Some(0.7), sediment_load_mgl: Some(200.0),
            groundwater_level_m: Some(1.8), reservoir_storage_pct: Some(65.0),
            flood_risk_level: Some(1), drought_index: Some(0.0),
        }),
        quality_score: Some(0.92),
        provenance: None,
    }
}

#[tokio::test]
async fn test_all_three_domains_store_and_index_consistently() {
    let orch = make_orchestrator();

    let s = orch.store_environmental_report(&soil_report("s1"), DataTier::Hot).await.unwrap();
    let o = orch.store_environmental_report(&ocean_report("o1"), DataTier::Hot).await.unwrap();
    let h = orch.store_environmental_report(&hydrology_report("h1"), DataTier::Hot).await.unwrap();

    assert_eq!(s, AdmissionDecision::Accepted);
    assert_eq!(o, AdmissionDecision::Accepted);
    assert_eq!(h, AdmissionDecision::Accepted);

    // Each domain has exactly 1 indexed entry
    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Soil).len(), 1);
    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Ocean).len(), 1);
    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Hydrology).len(), 1);
    assert_eq!(orch.domain_index_total(), 3);
}

#[tokio::test]
async fn test_multiple_reports_same_domain() {
    let orch = make_orchestrator();

    for i in 0..5 {
        let report = soil_report(&format!("soil-{i}"));
        let d = orch.store_environmental_report(&report, DataTier::Hot).await.unwrap();
        assert_eq!(d, AdmissionDecision::Accepted);
    }

    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Soil).len(), 5);
}

#[tokio::test]
async fn test_invalid_payload_rejected_not_indexed() {
    let orch = make_orchestrator();

    // Create a soil report with invalid pH
    let mut bad = soil_report("bad-soil");
    if let ReportPayload::Soil(ref mut p) = bad.payload {
        p.ph = Some(20.0); // out of range
    }

    let result = orch.store_environmental_report(&bad, DataTier::Hot).await;
    assert!(result.is_err()); // validation error
    assert_eq!(orch.domain_index_total(), 0); // nothing indexed
}

#[tokio::test]
async fn test_cross_tier_storage_parity() {
    let orch = make_orchestrator();

    // Store each domain in a different tier
    let s = orch.store_environmental_report(&soil_report("s-cold"), DataTier::Cold).await.unwrap();
    let o = orch.store_environmental_report(&ocean_report("o-hot"), DataTier::Hot).await.unwrap();
    let h = orch.store_environmental_report(&hydrology_report("h-archive"), DataTier::Archive).await.unwrap();

    assert_eq!(s, AdmissionDecision::Accepted);
    assert_eq!(o, AdmissionDecision::Accepted);
    assert_eq!(h, AdmissionDecision::Accepted);

    // All three are indexed regardless of tier
    assert_eq!(orch.domain_index_total(), 3);
}
```

**Step 2: Run integration tests**

Run: `cargo test -p wrfcoin-data-tiering --test domain_parity_integration`
Expected: PASS (4 tests)

**Step 3: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass

**Step 4: Commit**

```bash
git add crates/data-tiering/tests/domain_parity_integration.rs
git commit -m "test: add end-to-end integration tests for soil/ocean/hydrology index parity (#6)"
```

---

## Summary

| Task | What | Files |
|------|------|-------|
| 1 | SoilPayload::validate() | storage-common/src/types.rs |
| 2 | OceanPayload::validate() | storage-common/src/types.rs |
| 3 | HydrologyPayload::validate() | storage-common/src/types.rs |
| 4 | Wire payload validation into report validate() | storage-common/src/types.rs |
| 5 | DomainContentIndex | storage-common/src/domain_index.rs, lib.rs |
| 6 | Wire index into orchestrator | data-tiering/src/orchestrator.rs |
| 7 | Default retention policies | data-tiering/src/retention.rs |
| 8 | End-to-end integration tests | data-tiering/tests/domain_parity_integration.rs |

**Dependency note:** No external dependencies needed. Issue #6's stated dependency on "backend normalization pipelines" is already satisfied — the backend has NRCS soil, USGS hydrology, and environment-phase2 ocean services implemented.

**No blocking dependencies on other open issues:** Issues #1 (Filecoin deal index), #2 (migration content_id index), #5 (quality-retention), and #8 (public-interest access) are all independent and can proceed in parallel.
