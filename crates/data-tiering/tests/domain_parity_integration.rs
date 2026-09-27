//! Integration tests verifying index parity for soil, ocean, and hydrology
//! payload families across the data-tiering orchestrator.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use wrfcoin_data_tiering::{DataTieringOrchestrator, ProvenancePolicy, TieringConfig};
use wrfcoin_storage_common::{
    AdmissionDecision, ContentId, DataTier, EnvironmentalDomain, EnvironmentalReport, GeoLocation,
    HealthStatus, HydrologyPayload, Metadata, OceanPayload, ReportPayload, SoilPayload,
    StorageError, StorageProvider, StorageStats,
};

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
            for &b in data {
                h = h.wrapping_mul(33).wrapping_add(b as u64);
            }
            h
        });
        self.data
            .lock()
            .unwrap()
            .insert(hash.clone(), data.to_vec());
        Ok(ContentId {
            provider: self.name.clone(),
            hash,
        })
    }
    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        self.data
            .lock()
            .unwrap()
            .get(&id.hash)
            .cloned()
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
        Ok(HealthStatus {
            healthy: true,
            provider: self.name.clone(),
            latency_ms: Some(1),
            message: None,
        })
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
    })
    .unwrap();
    orch
}

fn soil_report(id: &str) -> EnvironmentalReport {
    EnvironmentalReport {
        report_id: id.into(),
        domain: EnvironmentalDomain::Soil,
        station_id: "soil-stn-1".into(),
        location: GeoLocation {
            latitude: 45.0,
            longitude: -93.0,
            altitude_m: None,
        },
        observed_at: 1000,
        submitted_at: 1001,
        payload: ReportPayload::Soil(SoilPayload {
            moisture_pct: Some(30.0),
            temperature_c: Some(18.0),
            ph: Some(6.5),
            organic_carbon_pct: Some(2.0),
            nitrogen_mgl: Some(10.0),
            phosphorus_mgl: Some(5.0),
            potassium_mgl: Some(100.0),
            conductivity_dsm: Some(0.4),
            depth_cm: Some(15.0),
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
        location: GeoLocation {
            latitude: 28.5,
            longitude: -89.2,
            altitude_m: None,
        },
        observed_at: 2000,
        submitted_at: 2001,
        payload: ReportPayload::Ocean(OceanPayload {
            sea_surface_temperature_c: Some(28.0),
            salinity_psu: Some(35.0),
            wave_height_m: Some(1.5),
            wave_period_s: Some(8.0),
            current_speed_ms: Some(0.3),
            current_direction_deg: Some(180.0_f64),
            chlorophyll_a_mgl: Some(0.2),
            dissolved_oxygen_mgl: Some(7.0),
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
        location: GeoLocation {
            latitude: 44.9,
            longitude: -93.1,
            altitude_m: Some(215.0),
        },
        observed_at: 3000,
        submitted_at: 3001,
        payload: ReportPayload::Hydrology(HydrologyPayload {
            river_discharge_m3s: Some(800.0),
            water_level_m: Some(3.5),
            flow_velocity_ms: Some(0.7),
            sediment_load_mgl: Some(200.0),
            groundwater_level_m: Some(1.8),
            reservoir_storage_pct: Some(65.0),
            flood_risk_level: Some(1),
            drought_index: Some(0.0),
        }),
        quality_score: Some(0.92),
        provenance: None,
    }
}

#[tokio::test]
async fn test_all_three_domains_store_and_index_consistently() {
    let orch = make_orchestrator();

    let s = orch
        .store_environmental_report(&soil_report("s1"), DataTier::Hot)
        .await
        .unwrap();
    let o = orch
        .store_environmental_report(&ocean_report("o1"), DataTier::Hot)
        .await
        .unwrap();
    let h = orch
        .store_environmental_report(&hydrology_report("h1"), DataTier::Hot)
        .await
        .unwrap();

    assert_eq!(s, AdmissionDecision::Accepted);
    assert_eq!(o, AdmissionDecision::Accepted);
    assert_eq!(h, AdmissionDecision::Accepted);

    // Each domain has exactly 1 indexed entry
    assert_eq!(orch.query_domain_index(&EnvironmentalDomain::Soil).len(), 1);
    assert_eq!(
        orch.query_domain_index(&EnvironmentalDomain::Ocean).len(),
        1
    );
    assert_eq!(
        orch.query_domain_index(&EnvironmentalDomain::Hydrology)
            .len(),
        1
    );
    assert_eq!(orch.domain_index_total(), 3);
}

#[tokio::test]
async fn test_multiple_reports_same_domain() {
    let orch = make_orchestrator();

    for i in 0..5 {
        let report = soil_report(&format!("soil-{i}"));
        let d = orch
            .store_environmental_report(&report, DataTier::Hot)
            .await
            .unwrap();
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
    let s = orch
        .store_environmental_report(&soil_report("s-cold"), DataTier::Cold)
        .await
        .unwrap();
    let o = orch
        .store_environmental_report(&ocean_report("o-hot"), DataTier::Hot)
        .await
        .unwrap();
    let h = orch
        .store_environmental_report(&hydrology_report("h-archive"), DataTier::Archive)
        .await
        .unwrap();

    assert_eq!(s, AdmissionDecision::Accepted);
    assert_eq!(o, AdmissionDecision::Accepted);
    assert_eq!(h, AdmissionDecision::Accepted);

    // All three are indexed regardless of tier
    assert_eq!(orch.domain_index_total(), 3);
}
