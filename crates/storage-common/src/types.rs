use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::meteo_compat as mt;

// Re-export canonical environmental types from meteo-types.
pub use mt::{EnvironmentalDomain, EnvironmentalReport, GeoLocation, ReportPayload};

// Re-export quality types so callers (e.g. data-tiering) can make quality-aware routing
// decisions without importing the compatibility module directly.
pub use mt::{DataQualityBand, DataQualityScore, WeatherObservation};

// Type aliases — keep existing names for all callers in this workspace.
pub type WeatherPayload = mt::WeatherObservation;
pub type AirQualityPayload = mt::AirQualityPayload;
pub type WaterQualityPayload = mt::WaterQualityPayload;
pub type WildfirePayload = mt::WildfirePayload;
pub type SoilPayload = mt::SoilPayload;
pub type OceanPayload = mt::OceanPayload;
pub type HydrologyPayload = mt::HydrologyPayload;

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

/// Typed storage metadata that carries canonical meteo-types fields alongside
/// the base [`Metadata`].
///
/// Use `StorageMetadata` instead of plain `Metadata` when storing environmental
/// observation data so that the `data-tiering` crate can make quality-aware and
/// domain-aware routing decisions without re-parsing stored content.
///
/// # Alignment with meteo-types
///
/// - [`EnvironmentalDomain`] identifies the observation category (Weather, AirQuality,
///   Soil, …) and drives the domain index in `DomainContentIndex`.
/// - [`DataQualityBand`] is derived from [`DataQualityScore::band()`] and directly maps
///   to tiering policy: `High` → Hot, `Borderline` → Warm, `WarningOnly` / `Low` → Cold
///   or Archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetadata {
    /// Base storage metadata (content-type, size, timestamp, freeform tags).
    pub base: Metadata,
    /// Environmental observation domain, if known.
    ///
    /// `None` for non-meteo content (e.g. raw binary blobs).
    pub domain: Option<EnvironmentalDomain>,
    /// Quality band classification for data-tiering routing decisions.
    ///
    /// `None` when quality scoring has not been performed.
    pub quality_band: Option<DataQualityBand>,
    /// Structured quality score (overall, range_score, consistency_score).
    ///
    /// `None` when quality scoring has not been performed.
    pub quality_score: Option<DataQualityScore>,
    /// Station or sensor identifier that produced the observation.
    ///
    /// Used to group related content in the domain index.
    pub station_id: Option<String>,
    /// Canonical report identifier from `EnvironmentalReport::report_id`.
    ///
    /// Enables content-addressed deduplication: the same report must not be
    /// stored twice.
    pub report_id: Option<String>,
}

impl StorageMetadata {
    /// Construct `StorageMetadata` from base metadata with no meteo context.
    pub fn from_base(base: Metadata) -> Self {
        Self {
            base,
            domain: None,
            quality_band: None,
            quality_score: None,
            station_id: None,
            report_id: None,
        }
    }

    /// Construct `StorageMetadata` from an `EnvironmentalReport`.
    ///
    /// The quality band and score are populated from `report.quality_score` if present.
    pub fn from_report(report: &EnvironmentalReport) -> Self {
        let quality_score_val = report.quality_score;
        let quality_score = quality_score_val.and_then(|s| {
            DataQualityScore::new(s, s, s).ok()
        });
        let quality_band = quality_score_val.map(DataQualityBand::from_score);

        let mut tags = HashMap::new();
        tags.insert("report_id".to_string(), report.report_id.clone());
        tags.insert("station_id".to_string(), report.station_id.clone());
        tags.insert("domain".to_string(), format!("{:?}", report.domain));

        Self {
            base: Metadata {
                content_type: "application/json".to_string(),
                size: 0, // caller fills in after serializing
                timestamp: report.submitted_at,
                tags,
            },
            domain: Some(report.domain),
            quality_band,
            quality_score,
            station_id: Some(report.station_id.clone()),
            report_id: Some(report.report_id.clone()),
        }
    }

    /// Returns `true` if the quality band is `High` or `Borderline`.
    ///
    /// Convenience method used by tiering logic to decide whether content should
    /// be stored in a warm-or-hotter tier.
    pub fn is_quality_acceptable(&self) -> bool {
        matches!(
            self.quality_band,
            Some(DataQualityBand::High) | Some(DataQualityBand::Borderline)
        )
    }

    /// Suggest a [`DataTier`] based on the quality band.
    ///
    /// | Band | Suggested tier |
    /// |------|---------------|
    /// | `High` | Hot |
    /// | `Borderline` | Warm |
    /// | `WarningOnly` | Cold |
    /// | `Low` / `None` | Archive |
    pub fn suggested_tier(&self) -> DataTier {
        match self.quality_band {
            Some(DataQualityBand::High) => DataTier::Hot,
            Some(DataQualityBand::Borderline) => DataTier::Warm,
            Some(DataQualityBand::WarningOnly) => DataTier::Cold,
            _ => DataTier::Archive,
        }
    }
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

/// Physical-range bounds validation for domain-specific payloads.
///
/// Implemented by storage-providers for the Soil, Ocean, and Hydrology payload
/// types from meteo-types.  Use [`validate_report_payload`] to dispatch by
/// variant.
pub trait PayloadBoundsValidate {
    fn validate_bounds(&self) -> Result<(), String>;
}

impl PayloadBoundsValidate for SoilPayload {
    fn validate_bounds(&self) -> Result<(), String> {
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

impl PayloadBoundsValidate for OceanPayload {
    fn validate_bounds(&self) -> Result<(), String> {
        if let Some(v) = self.salinity_psu {
            if !(0.0..=50.0).contains(&v) {
                return Err(format!("salinity_psu {v} is out of range [0.0, 50.0]"));
            }
        }
        if let Some(v) = self.current_direction_deg {
            if v > 360.0 {
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

impl PayloadBoundsValidate for HydrologyPayload {
    fn validate_bounds(&self) -> Result<(), String> {
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

/// Validate physical-range bounds for Soil, Ocean, and Hydrology payloads.
///
/// Returns `Ok(())` for all other payload domains (no bounds defined yet).
pub fn validate_report_payload(payload: &ReportPayload) -> Result<(), String> {
    match payload {
        ReportPayload::Soil(p) => p.validate_bounds(),
        ReportPayload::Ocean(p) => p.validate_bounds(),
        ReportPayload::Hydrology(p) => p.validate_bounds(),
        _ => Ok(()),
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
        assert_eq!(format!("{cid}"), "ipfs:QmTest123");
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
                humidity_percent: Some(65.0),
                pressure_hpa: Some(1013.25),
                wind_speed_ms: Some(3.2),
                wind_direction_deg: Some(270.0),
                precipitation_mm: None,
                dewpoint_c: None,
                visibility_m: Some(10_000.0),
                ..Default::default()
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
                active: Some(true),
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
                ..Default::default()
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
                current_direction_deg: Some(180.0_f64),
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
            assert_eq!(p.current_direction_deg, Some(180.0_f64));
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

    // ---- SoilPayload::validate_bounds() tests ----

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
        assert!(p.validate_bounds().is_ok());
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
        assert!(p.validate_bounds().is_ok());
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
        let err = p.validate_bounds().unwrap_err();
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
        let err = p.validate_bounds().unwrap_err();
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
        let err = p.validate_bounds().unwrap_err();
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
            current_direction_deg: Some(180.0_f64),
            chlorophyll_a_mgl: Some(0.25),
            dissolved_oxygen_mgl: Some(7.1),
            depth_m: Some(0.5),
        };
        assert!(p.validate_bounds().is_ok());
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
        assert!(p.validate_bounds().is_ok());
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
        let err = p.validate_bounds().unwrap_err();
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
        let err = p.validate_bounds().unwrap_err();
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
            current_direction_deg: Some(361.0_f64),
            chlorophyll_a_mgl: None,
            dissolved_oxygen_mgl: None,
            depth_m: None,
        };
        let err = p.validate_bounds().unwrap_err();
        assert!(err.contains("current_direction"), "expected error to mention 'current_direction', got: {err}");
    }

    // ---- HydrologyPayload::validate_bounds() tests ----

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
        assert!(p.validate_bounds().is_ok());
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
        assert!(p.validate_bounds().is_ok());
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
        let err = p.validate_bounds().unwrap_err();
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
        let err = p.validate_bounds().unwrap_err();
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
        let err = p.validate_bounds().unwrap_err();
        assert!(err.contains("river_discharge"), "expected error to mention 'river_discharge', got: {err}");
    }

    // ---- validate_report_payload() payload bounds tests ----

    #[test]
    fn test_report_validate_rejects_invalid_soil_payload() {
        let mut report = sample_soil_report();
        if let ReportPayload::Soil(ref mut p) = report.payload {
            p.ph = Some(20.0);
        }
        let err = validate_report_payload(&report.payload).unwrap_err();
        assert!(err.contains("ph"));
    }

    #[test]
    fn test_report_validate_rejects_invalid_ocean_payload() {
        let mut report = sample_ocean_report();
        if let ReportPayload::Ocean(ref mut p) = report.payload {
            p.salinity_psu = Some(-5.0);
        }
        let err = validate_report_payload(&report.payload).unwrap_err();
        assert!(err.contains("salinity"));
    }

    #[test]
    fn test_report_validate_rejects_invalid_hydrology_payload() {
        let mut report = sample_hydrology_report();
        if let ReportPayload::Hydrology(ref mut p) = report.payload {
            p.reservoir_storage_pct = Some(200.0);
        }
        let err = validate_report_payload(&report.payload).unwrap_err();
        assert!(err.contains("reservoir_storage"));
    }

    // ---- StorageMetadata tests ----

    fn sample_env_report() -> EnvironmentalReport {
        EnvironmentalReport {
            report_id: "sm-test-001".to_string(),
            domain: EnvironmentalDomain::Weather,
            station_id: "stn-sm-1".to_string(),
            location: GeoLocation { latitude: 40.0, longitude: -105.0, altitude_m: None },
            observed_at: 1_000_000,
            submitted_at: 1_000_001,
            payload: ReportPayload::Weather(WeatherPayload {
                temperature_c: Some(20.0),
                humidity_percent: Some(60.0),
                pressure_hpa: Some(1013.0),
                ..Default::default()
            }),
            quality_score: Some(0.92),
            provenance: None,
        }
    }

    #[test]
    fn test_storage_metadata_from_report_sets_domain() {
        let report = sample_env_report();
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.domain, Some(EnvironmentalDomain::Weather));
    }

    #[test]
    fn test_storage_metadata_from_report_sets_quality_band_high() {
        let report = sample_env_report(); // quality_score = 0.92 → High
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.quality_band, Some(DataQualityBand::High));
    }

    #[test]
    fn test_storage_metadata_from_report_borderline_quality() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.75); // 0.75 → Borderline
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.quality_band, Some(DataQualityBand::Borderline));
    }

    #[test]
    fn test_storage_metadata_from_report_warning_only_quality() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.55); // 0.55 → WarningOnly
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.quality_band, Some(DataQualityBand::WarningOnly));
    }

    #[test]
    fn test_storage_metadata_from_report_low_quality() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.30); // 0.30 → Low
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.quality_band, Some(DataQualityBand::Low));
    }

    #[test]
    fn test_storage_metadata_from_report_no_quality_score() {
        let mut report = sample_env_report();
        report.quality_score = None;
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.quality_band, None);
        assert_eq!(meta.quality_score, None);
    }

    #[test]
    fn test_storage_metadata_from_report_sets_station_and_report_id() {
        let report = sample_env_report();
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.station_id.as_deref(), Some("stn-sm-1"));
        assert_eq!(meta.report_id.as_deref(), Some("sm-test-001"));
    }

    #[test]
    fn test_storage_metadata_suggested_tier_high_is_hot() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.90);
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.suggested_tier(), DataTier::Hot);
    }

    #[test]
    fn test_storage_metadata_suggested_tier_borderline_is_warm() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.75);
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.suggested_tier(), DataTier::Warm);
    }

    #[test]
    fn test_storage_metadata_suggested_tier_warning_only_is_cold() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.60);
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.suggested_tier(), DataTier::Cold);
    }

    #[test]
    fn test_storage_metadata_suggested_tier_low_is_archive() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.30);
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.suggested_tier(), DataTier::Archive);
    }

    #[test]
    fn test_storage_metadata_suggested_tier_no_quality_is_archive() {
        let mut report = sample_env_report();
        report.quality_score = None;
        let meta = StorageMetadata::from_report(&report);
        assert_eq!(meta.suggested_tier(), DataTier::Archive);
    }

    #[test]
    fn test_storage_metadata_is_quality_acceptable_high() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.90);
        let meta = StorageMetadata::from_report(&report);
        assert!(meta.is_quality_acceptable());
    }

    #[test]
    fn test_storage_metadata_is_quality_acceptable_borderline() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.75);
        let meta = StorageMetadata::from_report(&report);
        assert!(meta.is_quality_acceptable());
    }

    #[test]
    fn test_storage_metadata_is_quality_not_acceptable_warning() {
        let mut report = sample_env_report();
        report.quality_score = Some(0.60);
        let meta = StorageMetadata::from_report(&report);
        assert!(!meta.is_quality_acceptable());
    }

    #[test]
    fn test_storage_metadata_from_base() {
        let base = Metadata {
            content_type: "application/octet-stream".to_string(),
            size: 1024,
            timestamp: 2000,
            tags: HashMap::new(),
        };
        let meta = StorageMetadata::from_base(base);
        assert!(meta.domain.is_none());
        assert!(meta.quality_band.is_none());
        assert!(meta.station_id.is_none());
        assert!(meta.report_id.is_none());
        assert_eq!(meta.base.size, 1024);
    }

    #[test]
    fn test_storage_metadata_serde_roundtrip() {
        let report = sample_env_report();
        let meta = StorageMetadata::from_report(&report);
        let json = serde_json::to_string(&meta).expect("serialize StorageMetadata");
        let decoded: StorageMetadata =
            serde_json::from_str(&json).expect("deserialize StorageMetadata");
        assert_eq!(decoded.domain, meta.domain);
        assert_eq!(decoded.quality_band, meta.quality_band);
        assert_eq!(decoded.station_id, meta.station_id);
    }
}
