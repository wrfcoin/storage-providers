pub mod compression;
pub mod error;
pub mod failover;
pub mod hashing;
pub mod health;
pub mod integrity;
pub mod provenance;
pub mod traits;
pub mod types;

pub use compression::{compress, decompress, is_compressed};
pub use error::StorageError;
pub use failover::{CircuitBreaker, CircuitState, FailoverMetrics, FailoverRouter};
pub use hashing::{arweave_content_id, filecoin_content_id, ipfs_content_id, sha256_digest};
pub use health::{AggregatedHealth, HealthAggregator, ProviderHealthEntry};
pub use integrity::IntegrityVerifier;
pub use provenance::{ProvenancePolicy, ProvenanceValidator};
pub use traits::{ContentAddressed, StorageProvider};
pub use types::{
    AdmissionDecision, AirQualityPayload, ArchiveReceipt, AvailabilitySlaStatus, ContentId,
    DataTier, EnvironmentalDomain, EnvironmentalReport, GeoLocation, HealthStatus,
    HydrologyPayload, Metadata, OceanPayload, ProvenanceChain, ProvenanceLink,
    ProvenanceValidationResult, PublicInterestAvailabilityPolicy, QuarantineRecord, ReportPayload,
    RetentionPolicy, SoilPayload, StorageReceipt, StorageStats, WaterQualityPayload,
    WeatherPayload, WildfirePayload,
};
