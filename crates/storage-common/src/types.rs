use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Represents a content identifier across storage providers.
/// Maps to a CID (IPFS), transaction ID (Arweave), or deal ID (Filecoin).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ContentId {
    /// The storage provider that owns this identifier (e.g. "ipfs", "filecoin", "arweave").
    pub provider: String,
    /// The content hash or identifier string.
    pub hash: String,
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider, self.hash)
    }
}

/// Metadata associated with stored content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// MIME type of the content (e.g. "application/octet-stream").
    pub content_type: String,
    /// Size of the content in bytes.
    pub size: u64,
    /// Unix timestamp when the content was created or ingested.
    pub timestamp: u64,
    /// Arbitrary key-value tags for classification.
    pub tags: HashMap<String, String>,
}

/// Receipt returned after a successful storage operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageReceipt {
    /// The content identifier assigned by the provider.
    pub content_id: ContentId,
    /// Name of the provider that stored the data.
    pub provider: String,
    /// Unix timestamp of the storage event.
    pub timestamp: u64,
    /// Size of the stored data in bytes.
    pub size: u64,
}

/// Health status of a storage provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Whether the provider is currently healthy and reachable.
    pub healthy: bool,
    /// Name of the provider.
    pub provider: String,
    /// Round-trip latency in milliseconds, if measured.
    pub latency_ms: Option<u64>,
    /// Human-readable status message.
    pub message: Option<String>,
}

/// Aggregate statistics for a storage provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageStats {
    /// Total number of store operations completed.
    pub total_stored: u64,
    /// Total number of retrieve operations completed.
    pub total_retrieved: u64,
    /// Total bytes written across all store operations.
    pub total_bytes: u64,
    /// Number of retrieval requests served from cache.
    pub cache_hits: u64,
    /// Number of retrieval requests that missed the cache.
    pub cache_misses: u64,
}

/// Classification tier for data lifecycle management.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataTier {
    /// Frequently accessed, lowest latency (e.g. IPFS pinned).
    Hot,
    /// Moderately accessed, balanced cost/latency.
    Warm,
    /// Infrequently accessed, lower cost (e.g. Filecoin deals).
    Cold,
    /// Permanent archival, highest durability (e.g. Arweave).
    Archive,
}

/// Signed provenance step binding an artifact hash to its parent lineage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceLink {
    /// Artifact hash for the current step.
    pub artifact_hash: String,
    /// Parent artifact hash (if present).
    pub parent_hash: Option<String>,
    /// Signer identity for this link.
    pub signer: String,
    /// Signature over the link payload.
    pub signature: String,
    /// Signature algorithm descriptor.
    pub algorithm: String,
    /// Unix timestamp of signature creation.
    pub signed_at: u64,
}

/// Provenance chain attached to incoming artifacts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceChain {
    /// Stable chain identifier.
    pub chain_id: String,
    /// Root hash expected at the chain head.
    pub root_hash: String,
    /// Ordered links from root -> artifact.
    pub links: Vec<ProvenanceLink>,
}

/// Result of provenance verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceValidationResult {
    pub valid: bool,
    pub reason: String,
    pub invalid_links: usize,
}

/// Routing decision used by ingestion logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AdmissionDecision {
    Accepted,
    Quarantined,
    Rejected,
}

/// Quarantine metadata for suspicious/synthetic content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuarantineRecord {
    pub quarantine_id: String,
    pub reason: String,
    pub detected_at: u64,
    pub review_required: bool,
    pub reinstated: bool,
}

/// Availability policy for public-interest report classes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicInterestAvailabilityPolicy {
    pub class: String,
    pub min_replicas: u8,
}

/// SLA status for public-interest availability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AvailabilitySlaStatus {
    pub class: String,
    pub target_replicas: u8,
    pub replicated_replicas: u8,
    pub read_failover_ready: bool,
    pub sla_met: bool,
}

/// Environmental report domain classification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EnvironmentalDomain {
    Weather,
    AirQuality,
    WaterQuality,
    Wildfire,
    Soil,
    Ocean,
    Hydrology,
}

/// Geographic location with optional altitude.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<f64>,
}

/// Weather observation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherPayload {
    pub temperature_c: Option<f64>,
    pub humidity_pct: Option<f64>,
    pub pressure_hpa: Option<f64>,
    pub wind_speed_ms: Option<f64>,
    pub wind_direction_deg: Option<u16>,
    pub precipitation_mm: Option<f64>,
    pub visibility_m: Option<f64>,
}

/// Air quality observation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirQualityPayload {
    pub aqi: Option<u16>,
    pub pm25_ugm3: Option<f64>,
    pub pm10_ugm3: Option<f64>,
    pub o3_ppb: Option<f64>,
    pub no2_ppb: Option<f64>,
    pub so2_ppb: Option<f64>,
    pub co_ppm: Option<f64>,
}

/// Water quality observation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaterQualityPayload {
    pub ph: Option<f64>,
    pub dissolved_oxygen_mgl: Option<f64>,
    pub turbidity_ntu: Option<f64>,
    pub conductivity_usm: Option<f64>,
    pub temperature_c: Option<f64>,
    pub total_dissolved_solids_mgl: Option<f64>,
}

/// Wildfire detection and status payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WildfirePayload {
    pub fire_radiative_power_mw: Option<f64>,
    pub burn_area_km2: Option<f64>,
    pub confidence_pct: Option<f64>,
    pub smoke_aod: Option<f64>,
    pub containment_pct: Option<f64>,
    pub active: bool,
}

/// Soil observation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoilPayload {
    pub moisture_pct: Option<f64>,
    pub temperature_c: Option<f64>,
    pub ph: Option<f64>,
    pub organic_carbon_pct: Option<f64>,
    pub nitrogen_mgl: Option<f64>,
    pub phosphorus_mgl: Option<f64>,
    pub potassium_mgl: Option<f64>,
    pub conductivity_dsm: Option<f64>,
    pub depth_cm: Option<f64>,
}

impl SoilPayload {
    /// Validate physical bounds of soil observation fields.
    ///
    /// Skips any field that is `None`. Returns an error describing the first
    /// out-of-range field encountered.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(v) = self.moisture_pct {
            if !(0.0..=100.0).contains(&v) {
                return Err(format!("moisture_pct {v} is out of range [0.0, 100.0]"));
            }
        }
        if let Some(v) = self.ph {
            if !(0.0..=14.0).contains(&v) {
                return Err(format!("ph {v} is out of range [0.0, 14.0]"));
            }
        }
        if let Some(v) = self.organic_carbon_pct {
            if !(0.0..=100.0).contains(&v) {
                return Err(format!("organic_carbon_pct {v} is out of range [0.0, 100.0]"));
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

/// Ocean observation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OceanPayload {
    pub sea_surface_temperature_c: Option<f64>,
    pub salinity_psu: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub current_speed_ms: Option<f64>,
    pub current_direction_deg: Option<u16>,
    pub chlorophyll_a_mgl: Option<f64>,
    pub dissolved_oxygen_mgl: Option<f64>,
    pub depth_m: Option<f64>,
}

impl OceanPayload {
    /// Validate physical bounds of ocean observation fields.
    ///
    /// Skips any field that is `None`. Returns an error describing the first
    /// out-of-range field encountered.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(v) = self.salinity_psu {
            if !(0.0..=50.0).contains(&v) {
                return Err(format!("salinity_psu {v} is out of range [0.0, 50.0]"));
            }
        }
        if let Some(v) = self.current_direction_deg {
            if v > 360 {
                return Err(format!("current_direction_deg {v} is out of range [0, 360]"));
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

/// Hydrology observation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HydrologyPayload {
    pub river_discharge_m3s: Option<f64>,
    pub water_level_m: Option<f64>,
    pub flow_velocity_ms: Option<f64>,
    pub sediment_load_mgl: Option<f64>,
    pub groundwater_level_m: Option<f64>,
    pub reservoir_storage_pct: Option<f64>,
    pub flood_risk_level: Option<u8>,
    pub drought_index: Option<f64>,
}

impl HydrologyPayload {
    /// Validate physical bounds of hydrology observation fields.
    ///
    /// Skips any field that is `None`. Returns an error describing the first
    /// out-of-range field encountered.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(v) = self.reservoir_storage_pct {
            if !(0.0..=100.0).contains(&v) {
                return Err(format!(
                    "reservoir_storage_pct {v} is out of range [0.0, 100.0]"
                ));
            }
        }
        if let Some(v) = self.flood_risk_level {
            if v > 5 {
                return Err(format!("flood_risk_level {v} is out of range [0, 5]"));
            }
        }
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
        Ok(())
    }
}

/// Domain-specific payload variants for environmental reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReportPayload {
    Weather(WeatherPayload),
    AirQuality(AirQualityPayload),
    WaterQuality(WaterQualityPayload),
    Wildfire(WildfirePayload),
    Soil(SoilPayload),
    Ocean(OceanPayload),
    Hydrology(HydrologyPayload),
}

impl ReportPayload {
    /// Return the domain that corresponds to this payload variant.
    pub fn domain(&self) -> EnvironmentalDomain {
        match self {
            ReportPayload::Weather(_) => EnvironmentalDomain::Weather,
            ReportPayload::AirQuality(_) => EnvironmentalDomain::AirQuality,
            ReportPayload::WaterQuality(_) => EnvironmentalDomain::WaterQuality,
            ReportPayload::Wildfire(_) => EnvironmentalDomain::Wildfire,
            ReportPayload::Soil(_) => EnvironmentalDomain::Soil,
            ReportPayload::Ocean(_) => EnvironmentalDomain::Ocean,
            ReportPayload::Hydrology(_) => EnvironmentalDomain::Hydrology,
        }
    }
}

/// Canonical environmental report payload covering multiple observation domains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentalReport {
    /// Unique identifier for this report.
    pub report_id: String,
    /// Domain classification (must match `payload` variant).
    pub domain: EnvironmentalDomain,
    /// Identifier of the originating station or sensor.
    pub station_id: String,
    /// Geographic location of the observation.
    pub location: GeoLocation,
    /// Unix timestamp when the observation was made.
    pub observed_at: u64,
    /// Unix timestamp when the report was submitted.
    pub submitted_at: u64,
    /// Domain-specific measurement payload.
    pub payload: ReportPayload,
    /// Optional data quality score in the range [0.0, 1.0].
    pub quality_score: Option<f64>,
    /// Optional provenance chain for attestation.
    pub provenance: Option<ProvenanceChain>,
}

impl EnvironmentalReport {
    /// Validate report consistency.
    ///
    /// Checks that required string fields are non-empty and that `domain`
    /// matches the `payload` variant.
    pub fn validate(&self) -> Result<(), String> {
        if self.report_id.is_empty() {
            return Err("empty report_id".into());
        }
        if self.station_id.is_empty() {
            return Err("empty station_id".into());
        }
        if self.domain != self.payload.domain() {
            return Err(format!(
                "domain {:?} does not match payload variant {:?}",
                self.domain,
                self.payload.domain()
            ));
        }
        if let Some(score) = self.quality_score {
            if !(0.0..=1.0).contains(&score) {
                return Err(format!(
                    "quality_score {score} is out of range [0.0, 1.0]"
                ));
            }
        }
        // Validate domain-specific payload bounds.
        match &self.payload {
            ReportPayload::Soil(p) => p.validate()?,
            ReportPayload::Ocean(p) => p.validate()?,
            ReportPayload::Hydrology(p) => p.validate()?,
            _ => {} // Other domains: no bounds validation yet
        }
        Ok(())
    }
}

/// Receipt from archiving data to multiple providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveReceipt {
    /// Canonical content ID (blake3 hash of original data).
    pub canonical_cid: String,
    /// Per-provider storage results.
    pub provider_receipts: Vec<StorageReceipt>,
    /// Timestamp of archive operation.
    pub archived_at: u64,
    /// Total providers attempted.
    pub providers_attempted: usize,
    /// Total providers succeeded.
    pub providers_succeeded: usize,
    /// Result of provenance validation (if performed).
    pub provenance_result: Option<ProvenanceValidationResult>,
    /// Admission decision (if provenance gate was active).
    pub admission_decision: Option<AdmissionDecision>,
}

/// Retention policy for archived data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Minimum number of provider copies to maintain.
    pub min_copies: usize,
    /// Maximum age in seconds before expiry (0 = never expires).
    pub max_age_secs: u64,
    /// Data tier this policy applies to.
    pub tier: DataTier,
    /// Artifact class (e.g., "forecast", "observation", "analysis").
    pub artifact_class: String,
    /// Quality tier minimum (0.0-1.0, data below this gets shorter retention).
    pub min_quality_score: f64,
}

impl RetentionPolicy {
    /// Create a new retention policy with sensible defaults.
    pub fn new(artifact_class: &str, tier: DataTier, min_copies: usize) -> Self {
        Self {
            min_copies,
            max_age_secs: 0,
            tier,
            artifact_class: artifact_class.to_string(),
            min_quality_score: 0.0,
        }
    }

    /// Check whether data should be retained given its age and quality score.
    ///
    /// Returns `true` if the data should be kept:
    /// - Quality score must be >= `min_quality_score`
    /// - Age must be <= `max_age_secs` (or `max_age_secs == 0` means never expires)
    pub fn should_retain(&self, age_secs: u64, quality_score: f64) -> bool {
        if quality_score < self.min_quality_score {
            return false;
        }
        if self.max_age_secs == 0 {
            return true;
        }
        age_secs <= self.max_age_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_id_display() {
        let cid = ContentId {
            provider: "ipfs".to_string(),
            hash: "QmTest123".to_string(),
        };
        assert_eq!(format!("{}", cid), "ipfs:QmTest123");
    }

    #[test]
    fn test_content_id_equality() {
        let a = ContentId { provider: "ipfs".to_string(), hash: "abc".to_string() };
        let b = ContentId { provider: "ipfs".to_string(), hash: "abc".to_string() };
        let c = ContentId { provider: "ipfs".to_string(), hash: "def".to_string() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_metadata_tags() {
        let mut tags = HashMap::new();
        tags.insert("type".to_string(), "weather".to_string());
        let meta = Metadata {
            content_type: "application/json".to_string(),
            size: 1024,
            timestamp: 1000,
            tags,
        };
        assert_eq!(meta.tags.get("type").unwrap(), "weather");
        assert_eq!(meta.size, 1024);
    }

    #[test]
    fn test_storage_receipt() {
        let receipt = StorageReceipt {
            content_id: ContentId { provider: "ipfs".to_string(), hash: "Qm123".to_string() },
            provider: "ipfs".to_string(),
            timestamp: 1000,
            size: 512,
        };
        assert_eq!(receipt.size, 512);
        assert_eq!(receipt.provider, "ipfs");
    }

    #[test]
    fn test_health_status() {
        let status = HealthStatus {
            healthy: true,
            provider: "ipfs".to_string(),
            latency_ms: Some(5),
            message: None,
        };
        assert!(status.healthy);
        assert_eq!(status.latency_ms, Some(5));
    }

    #[test]
    fn test_storage_stats_default() {
        let stats = StorageStats::default();
        assert_eq!(stats.total_stored, 0);
        assert_eq!(stats.total_retrieved, 0);
        assert_eq!(stats.total_bytes, 0);
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.cache_misses, 0);
    }

    #[test]
    fn test_data_tier_variants() {
        let tiers = [DataTier::Hot, DataTier::Warm, DataTier::Cold, DataTier::Archive];
        assert_eq!(tiers.len(), 4);
        assert_ne!(DataTier::Hot, DataTier::Cold);
    }

    #[test]
    fn test_provenance_chain_roundtrip() {
        let chain = ProvenanceChain {
            chain_id: "chain-a".to_string(),
            root_hash: "abc".to_string(),
            links: vec![ProvenanceLink {
                artifact_hash: "abc".to_string(),
                parent_hash: None,
                signer: "signer-1".to_string(),
                signature: "sig".to_string(),
                algorithm: "ed25519".to_string(),
                signed_at: 123,
            }],
        };
        let json = serde_json::to_string(&chain).expect("serialize chain");
        let decoded: ProvenanceChain = serde_json::from_str(&json).expect("deserialize chain");
        assert_eq!(decoded.chain_id, "chain-a");
        assert_eq!(decoded.links.len(), 1);
    }

    #[test]
    fn test_availability_sla_status() {
        let status = AvailabilitySlaStatus {
            class: "public_interest.weather".to_string(),
            target_replicas: 3,
            replicated_replicas: 2,
            read_failover_ready: true,
            sla_met: false,
        };
        assert_eq!(status.class, "public_interest.weather");
        assert!(!status.sla_met);
    }

    // ---- EnvironmentalReport tests ----

    fn sample_weather_report() -> EnvironmentalReport {
        EnvironmentalReport {
            report_id: "rpt-001".to_string(),
            domain: EnvironmentalDomain::Weather,
            station_id: "stn-42".to_string(),
            location: GeoLocation {
                latitude: 37.7749,
                longitude: -122.4194,
                altitude_m: Some(10.0),
            },
            observed_at: 1_000_000,
            submitted_at: 1_000_001,
            payload: ReportPayload::Weather(WeatherPayload {
                temperature_c: Some(21.5),
                humidity_pct: Some(65.0),
                pressure_hpa: Some(1013.25),
                wind_speed_ms: Some(3.2),
                wind_direction_deg: Some(270),
                precipitation_mm: None,
                visibility_m: Some(10_000.0),
            }),
            quality_score: Some(0.95),
            provenance: None,
        }
    }

    #[test]
    fn test_environmental_report_construction() {
        let report = sample_weather_report();
        assert_eq!(report.report_id, "rpt-001");
        assert_eq!(report.domain, EnvironmentalDomain::Weather);
        assert_eq!(report.station_id, "stn-42");
        assert!((report.location.latitude - 37.7749).abs() < f64::EPSILON);
        assert_eq!(report.quality_score, Some(0.95));
    }

    #[test]
    fn test_environmental_report_serde_roundtrip() {
        let report = sample_weather_report();
        let json = serde_json::to_string(&report).expect("serialize");
        let decoded: EnvironmentalReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.report_id, report.report_id);
        assert_eq!(decoded.domain, EnvironmentalDomain::Weather);
        assert_eq!(decoded.station_id, report.station_id);
        assert_eq!(decoded.quality_score, report.quality_score);
    }

    #[test]
    fn test_environmental_report_validate_ok() {
        let report = sample_weather_report();
        assert!(report.validate().is_ok());
    }

    #[test]
    fn test_environmental_report_validate_empty_report_id() {
        let mut report = sample_weather_report();
        report.report_id = "".to_string();
        let err = report.validate().unwrap_err();
        assert!(err.contains("report_id"));
    }

    #[test]
    fn test_environmental_report_validate_empty_station_id() {
        let mut report = sample_weather_report();
        report.station_id = "".to_string();
        let err = report.validate().unwrap_err();
        assert!(err.contains("station_id"));
    }

    #[test]
    fn test_environmental_report_validate_domain_payload_mismatch() {
        // domain says AirQuality but payload is Weather
        let mut report = sample_weather_report();
        report.domain = EnvironmentalDomain::AirQuality;
        let err = report.validate().unwrap_err();
        assert!(err.contains("domain"));
    }

    #[test]
    fn test_environmental_report_validate_quality_score_out_of_range() {
        let mut report = sample_weather_report();
        report.quality_score = Some(1.5);
        let err = report.validate().unwrap_err();
        assert!(err.contains("quality_score"));
    }

    #[test]
    fn test_air_quality_report_serde_roundtrip() {
        let report = EnvironmentalReport {
            report_id: "aq-001".to_string(),
            domain: EnvironmentalDomain::AirQuality,
            station_id: "aq-stn-1".to_string(),
            location: GeoLocation { latitude: 34.0, longitude: -118.0, altitude_m: None },
            observed_at: 2_000_000,
            submitted_at: 2_000_001,
            payload: ReportPayload::AirQuality(AirQualityPayload {
                aqi: Some(85),
                pm25_ugm3: Some(12.4),
                pm10_ugm3: Some(25.0),
                o3_ppb: Some(40.0),
                no2_ppb: Some(15.0),
                so2_ppb: None,
                co_ppm: Some(0.4),
            }),
            quality_score: Some(0.8),
            provenance: None,
        };
        assert!(report.validate().is_ok());
        let json = serde_json::to_string(&report).expect("serialize");
        let decoded: EnvironmentalReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.domain, EnvironmentalDomain::AirQuality);
    }

    #[test]
    fn test_water_quality_report_validate_ok() {
        let report = EnvironmentalReport {
            report_id: "wq-001".to_string(),
            domain: EnvironmentalDomain::WaterQuality,
            station_id: "wq-stn-1".to_string(),
            location: GeoLocation { latitude: 47.0, longitude: -122.0, altitude_m: None },
            observed_at: 3_000_000,
            submitted_at: 3_000_001,
            payload: ReportPayload::WaterQuality(WaterQualityPayload {
                ph: Some(7.2),
                dissolved_oxygen_mgl: Some(8.5),
                turbidity_ntu: Some(1.2),
                conductivity_usm: Some(320.0),
                temperature_c: Some(15.0),
                total_dissolved_solids_mgl: Some(210.0),
            }),
            quality_score: None,
            provenance: None,
        };
        assert!(report.validate().is_ok());
    }

    #[test]
    fn test_wildfire_report_validate_ok() {
        let report = EnvironmentalReport {
            report_id: "wf-001".to_string(),
            domain: EnvironmentalDomain::Wildfire,
            station_id: "sat-modis-1".to_string(),
            location: GeoLocation { latitude: 38.5, longitude: -120.5, altitude_m: None },
            observed_at: 4_000_000,
            submitted_at: 4_000_001,
            payload: ReportPayload::Wildfire(WildfirePayload {
                fire_radiative_power_mw: Some(250.0),
                burn_area_km2: Some(12.5),
                confidence_pct: Some(90.0),
                smoke_aod: Some(0.45),
                containment_pct: Some(30.0),
                active: true,
            }),
            quality_score: Some(1.0),
            provenance: None,
        };
        assert!(report.validate().is_ok());
    }

    #[test]
    fn test_wildfire_domain_payload_mismatch_detection() {
        let report = EnvironmentalReport {
            report_id: "mismatch-001".to_string(),
            domain: EnvironmentalDomain::Wildfire,
            station_id: "stn-1".to_string(),
            location: GeoLocation { latitude: 0.0, longitude: 0.0, altitude_m: None },
            observed_at: 1,
            submitted_at: 2,
            // Wildfire domain but Weather payload — should fail validation
            payload: ReportPayload::Weather(WeatherPayload {
                temperature_c: Some(30.0),
                humidity_pct: None,
                pressure_hpa: None,
                wind_speed_ms: None,
                wind_direction_deg: None,
                precipitation_mm: None,
                visibility_m: None,
            }),
            quality_score: None,
            provenance: None,
        };
        let err = report.validate().unwrap_err();
        assert!(err.contains("domain"));
    }

    // ---- Soil payload tests ----

    fn sample_soil_report() -> EnvironmentalReport {
        EnvironmentalReport {
            report_id: "soil-001".to_string(),
            domain: EnvironmentalDomain::Soil,
            station_id: "soil-stn-1".to_string(),
            location: GeoLocation { latitude: 45.0, longitude: -93.0, altitude_m: Some(300.0) },
            observed_at: 5_000_000,
            submitted_at: 5_000_001,
            payload: ReportPayload::Soil(SoilPayload {
                moisture_pct: Some(32.5),
                temperature_c: Some(18.0),
                ph: Some(6.8),
                organic_carbon_pct: Some(2.1),
                nitrogen_mgl: Some(15.0),
                phosphorus_mgl: Some(8.5),
                potassium_mgl: Some(120.0),
                conductivity_dsm: Some(0.45),
                depth_cm: Some(10.0),
            }),
            quality_score: Some(0.9),
            provenance: None,
        }
    }

    #[test]
    fn test_soil_payload_construction() {
        let report = sample_soil_report();
        assert_eq!(report.report_id, "soil-001");
        assert_eq!(report.domain, EnvironmentalDomain::Soil);
        if let ReportPayload::Soil(ref p) = report.payload {
            assert_eq!(p.moisture_pct, Some(32.5));
            assert_eq!(p.ph, Some(6.8));
            assert_eq!(p.depth_cm, Some(10.0));
        } else {
            panic!("expected Soil payload variant");
        }
    }

    #[test]
    fn test_soil_report_serde_roundtrip() {
        let report = sample_soil_report();
        let json = serde_json::to_string(&report).expect("serialize soil report");
        let decoded: EnvironmentalReport = serde_json::from_str(&json).expect("deserialize soil report");
        assert_eq!(decoded.report_id, report.report_id);
        assert_eq!(decoded.domain, EnvironmentalDomain::Soil);
        assert_eq!(decoded.quality_score, report.quality_score);
    }

    #[test]
    fn test_soil_report_validate_ok() {
        let report = sample_soil_report();
        assert!(report.validate().is_ok());
    }

    #[test]
    fn test_soil_domain_payload_mismatch_detection() {
        let mut report = sample_soil_report();
        // Switch domain to Ocean while keeping Soil payload
        report.domain = EnvironmentalDomain::Ocean;
        let err = report.validate().unwrap_err();
        assert!(err.contains("domain"));
    }

    // ---- Ocean payload tests ----

    fn sample_ocean_report() -> EnvironmentalReport {
        EnvironmentalReport {
            report_id: "ocean-001".to_string(),
            domain: EnvironmentalDomain::Ocean,
            station_id: "buoy-7".to_string(),
            location: GeoLocation { latitude: 28.5, longitude: -89.2, altitude_m: None },
            observed_at: 6_000_000,
            submitted_at: 6_000_001,
            payload: ReportPayload::Ocean(OceanPayload {
                sea_surface_temperature_c: Some(28.3),
                salinity_psu: Some(35.2),
                wave_height_m: Some(1.8),
                wave_period_s: Some(9.5),
                current_speed_ms: Some(0.4),
                current_direction_deg: Some(180),
                chlorophyll_a_mgl: Some(0.25),
                dissolved_oxygen_mgl: Some(7.1),
                depth_m: Some(0.5),
            }),
            quality_score: Some(0.85),
            provenance: None,
        }
    }

    #[test]
    fn test_ocean_payload_construction() {
        let report = sample_ocean_report();
        assert_eq!(report.report_id, "ocean-001");
        assert_eq!(report.domain, EnvironmentalDomain::Ocean);
        if let ReportPayload::Ocean(ref p) = report.payload {
            assert_eq!(p.sea_surface_temperature_c, Some(28.3));
            assert_eq!(p.salinity_psu, Some(35.2));
            assert_eq!(p.current_direction_deg, Some(180));
        } else {
            panic!("expected Ocean payload variant");
        }
    }

    #[test]
    fn test_ocean_report_serde_roundtrip() {
        let report = sample_ocean_report();
        let json = serde_json::to_string(&report).expect("serialize ocean report");
        let decoded: EnvironmentalReport = serde_json::from_str(&json).expect("deserialize ocean report");
        assert_eq!(decoded.report_id, report.report_id);
        assert_eq!(decoded.domain, EnvironmentalDomain::Ocean);
        assert_eq!(decoded.quality_score, report.quality_score);
    }

    #[test]
    fn test_ocean_report_validate_ok() {
        let report = sample_ocean_report();
        assert!(report.validate().is_ok());
    }

    #[test]
    fn test_ocean_domain_payload_mismatch_detection() {
        let mut report = sample_ocean_report();
        // Switch domain to Hydrology while keeping Ocean payload
        report.domain = EnvironmentalDomain::Hydrology;
        let err = report.validate().unwrap_err();
        assert!(err.contains("domain"));
    }

    // ---- Hydrology payload tests ----

    fn sample_hydrology_report() -> EnvironmentalReport {
        EnvironmentalReport {
            report_id: "hydro-001".to_string(),
            domain: EnvironmentalDomain::Hydrology,
            station_id: "gauge-mississippi-1".to_string(),
            location: GeoLocation { latitude: 44.9, longitude: -93.1, altitude_m: Some(215.0) },
            observed_at: 7_000_000,
            submitted_at: 7_000_001,
            payload: ReportPayload::Hydrology(HydrologyPayload {
                river_discharge_m3s: Some(1250.0),
                water_level_m: Some(4.3),
                flow_velocity_ms: Some(0.85),
                sediment_load_mgl: Some(320.0),
                groundwater_level_m: Some(2.1),
                reservoir_storage_pct: Some(72.0),
                flood_risk_level: Some(2),
                drought_index: Some(-0.5),
            }),
            quality_score: Some(0.92),
            provenance: None,
        }
    }

    #[test]
    fn test_hydrology_payload_construction() {
        let report = sample_hydrology_report();
        assert_eq!(report.report_id, "hydro-001");
        assert_eq!(report.domain, EnvironmentalDomain::Hydrology);
        if let ReportPayload::Hydrology(ref p) = report.payload {
            assert_eq!(p.river_discharge_m3s, Some(1250.0));
            assert_eq!(p.flood_risk_level, Some(2));
            assert_eq!(p.drought_index, Some(-0.5));
        } else {
            panic!("expected Hydrology payload variant");
        }
    }

    #[test]
    fn test_hydrology_report_serde_roundtrip() {
        let report = sample_hydrology_report();
        let json = serde_json::to_string(&report).expect("serialize hydrology report");
        let decoded: EnvironmentalReport =
            serde_json::from_str(&json).expect("deserialize hydrology report");
        assert_eq!(decoded.report_id, report.report_id);
        assert_eq!(decoded.domain, EnvironmentalDomain::Hydrology);
        assert_eq!(decoded.quality_score, report.quality_score);
    }

    #[test]
    fn test_hydrology_report_validate_ok() {
        let report = sample_hydrology_report();
        assert!(report.validate().is_ok());
    }

    #[test]
    fn test_hydrology_domain_payload_mismatch_detection() {
        let mut report = sample_hydrology_report();
        // Switch domain to Soil while keeping Hydrology payload
        report.domain = EnvironmentalDomain::Soil;
        let err = report.validate().unwrap_err();
        assert!(err.contains("domain"));
    }

    // ---- SoilPayload::validate() tests ----

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
            moisture_pct: None,
            temperature_c: None,
            ph: None,
            organic_carbon_pct: None,
            nitrogen_mgl: None,
            phosphorus_mgl: None,
            potassium_mgl: None,
            conductivity_dsm: None,
            depth_cm: None,
        };
        assert!(p.validate().is_ok());
    }

    #[test]
    fn test_soil_payload_validate_ph_out_of_range() {
        let p = SoilPayload {
            moisture_pct: None,
            temperature_c: None,
            ph: Some(15.0),
            organic_carbon_pct: None,
            nitrogen_mgl: None,
            phosphorus_mgl: None,
            potassium_mgl: None,
            conductivity_dsm: None,
            depth_cm: None,
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("ph"), "expected error to mention 'ph', got: {err}");
    }

    #[test]
    fn test_soil_payload_validate_moisture_out_of_range() {
        let p = SoilPayload {
            moisture_pct: Some(150.0),
            temperature_c: None,
            ph: None,
            organic_carbon_pct: None,
            nitrogen_mgl: None,
            phosphorus_mgl: None,
            potassium_mgl: None,
            conductivity_dsm: None,
            depth_cm: None,
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("moisture"), "expected error to mention 'moisture', got: {err}");
    }

    #[test]
    fn test_soil_payload_validate_negative_depth() {
        let p = SoilPayload {
            moisture_pct: None,
            temperature_c: None,
            ph: None,
            organic_carbon_pct: None,
            nitrogen_mgl: None,
            phosphorus_mgl: None,
            potassium_mgl: None,
            conductivity_dsm: None,
            depth_cm: Some(-5.0),
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("depth"), "expected error to mention 'depth', got: {err}");
    }

    // ---- OceanPayload::validate() tests ----

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
            sea_surface_temperature_c: None,
            salinity_psu: None,
            wave_height_m: None,
            wave_period_s: None,
            current_speed_ms: None,
            current_direction_deg: None,
            chlorophyll_a_mgl: None,
            dissolved_oxygen_mgl: None,
            depth_m: None,
        };
        assert!(p.validate().is_ok());
    }

    #[test]
    fn test_ocean_payload_validate_salinity_out_of_range() {
        let p = OceanPayload {
            sea_surface_temperature_c: None,
            salinity_psu: Some(60.0),
            wave_height_m: None,
            wave_period_s: None,
            current_speed_ms: None,
            current_direction_deg: None,
            chlorophyll_a_mgl: None,
            dissolved_oxygen_mgl: None,
            depth_m: None,
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("salinity"), "expected error to mention 'salinity', got: {err}");
    }

    #[test]
    fn test_ocean_payload_validate_negative_wave_height() {
        let p = OceanPayload {
            sea_surface_temperature_c: None,
            salinity_psu: None,
            wave_height_m: Some(-1.0),
            wave_period_s: None,
            current_speed_ms: None,
            current_direction_deg: None,
            chlorophyll_a_mgl: None,
            dissolved_oxygen_mgl: None,
            depth_m: None,
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("wave_height"), "expected error to mention 'wave_height', got: {err}");
    }

    #[test]
    fn test_ocean_payload_validate_direction_over_360() {
        let p = OceanPayload {
            sea_surface_temperature_c: None,
            salinity_psu: None,
            wave_height_m: None,
            wave_period_s: None,
            current_speed_ms: None,
            current_direction_deg: Some(361),
            chlorophyll_a_mgl: None,
            dissolved_oxygen_mgl: None,
            depth_m: None,
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("current_direction"), "expected error to mention 'current_direction', got: {err}");
    }

    // ---- HydrologyPayload::validate() tests ----

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
            river_discharge_m3s: None,
            water_level_m: None,
            flow_velocity_ms: None,
            sediment_load_mgl: None,
            groundwater_level_m: None,
            reservoir_storage_pct: None,
            flood_risk_level: None,
            drought_index: None,
        };
        assert!(p.validate().is_ok());
    }

    #[test]
    fn test_hydrology_payload_validate_reservoir_over_100() {
        let p = HydrologyPayload {
            river_discharge_m3s: None,
            water_level_m: None,
            flow_velocity_ms: None,
            sediment_load_mgl: None,
            groundwater_level_m: None,
            reservoir_storage_pct: Some(110.0),
            flood_risk_level: None,
            drought_index: None,
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("reservoir_storage"), "expected error to mention 'reservoir_storage', got: {err}");
    }

    #[test]
    fn test_hydrology_payload_validate_flood_risk_over_5() {
        let p = HydrologyPayload {
            river_discharge_m3s: None,
            water_level_m: None,
            flow_velocity_ms: None,
            sediment_load_mgl: None,
            groundwater_level_m: None,
            reservoir_storage_pct: None,
            flood_risk_level: Some(6),
            drought_index: None,
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("flood_risk"), "expected error to mention 'flood_risk', got: {err}");
    }

    #[test]
    fn test_hydrology_payload_validate_negative_discharge() {
        let p = HydrologyPayload {
            river_discharge_m3s: Some(-10.0),
            water_level_m: None,
            flow_velocity_ms: None,
            sediment_load_mgl: None,
            groundwater_level_m: None,
            reservoir_storage_pct: None,
            flood_risk_level: None,
            drought_index: None,
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("river_discharge"), "expected error to mention 'river_discharge', got: {err}");
    }

    // ---- EnvironmentalReport::validate() payload wiring tests ----

    #[test]
    fn test_report_validate_rejects_invalid_soil_payload() {
        let mut report = sample_soil_report();
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
}
