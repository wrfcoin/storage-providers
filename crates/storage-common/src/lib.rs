pub mod cf_metadata;
pub mod chunking;
pub mod compression;
pub mod deduplication;
pub mod domain_index;
pub mod error;
pub mod failover;
pub mod hashing;
pub mod health;
pub mod integrity;
pub mod provenance;
pub mod traits;
pub mod types;

pub use cf_metadata::{CfGlobalAttributes, CfMetadata, CfRegistry, CfVariable};
pub use chunking::{AdaptiveChunker, ChunkManifest, ChunkSizeHeuristics, CompressionStrategy};
pub use compression::{compress, decompress, is_compressed};
pub use deduplication::{DedupEntry, DeduplicationRegistry};
pub use domain_index::{DomainContentIndex, IndexEntry};
pub use error::StorageError;
pub use failover::{CircuitBreaker, CircuitState, FailoverMetrics, FailoverRouter};
pub use hashing::{arweave_content_id, filecoin_content_id, ipfs_content_id, observation_content_id, sha256_digest};
pub use health::{AggregatedHealth, HealthAggregator, ProviderHealthEntry};
pub use integrity::IntegrityVerifier;
pub use provenance::{ProvenancePolicy, ProvenanceValidator};
pub use traits::{ContentAddressed, StorageProvider};
pub use types::{
    AdmissionDecision, AirQualityPayload, ArchiveReceipt, AvailabilitySlaStatus, ContentId,
    DataQualityBand, DataQualityScore, DataTier, EnvironmentalDomain, EnvironmentalReport,
    GeoLocation, HealthStatus, HydrologyPayload, Metadata, OceanPayload, PayloadBoundsValidate,
    ProvenanceChain, ProvenanceLink, ProvenanceValidationResult, PublicInterestAvailabilityPolicy,
    QuarantineRecord, ReportPayload, RetentionPolicy, SoilPayload, StorageMetadata, StorageReceipt,
    StorageStats, WaterQualityPayload, WeatherObservation, WeatherPayload, WildfirePayload,
    validate_report_payload,
};
