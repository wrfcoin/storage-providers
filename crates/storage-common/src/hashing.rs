use sha2::{Digest, Sha256};

use crate::types::{ContentId, EnvironmentalReport};

/// Compute a SHA-256 digest and return raw bytes.
pub fn sha256_digest(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Generate an IPFS CIDv0-style content ID.
///
/// Format: `Qm` + base58(0x12 0x20 + SHA-256(data))
/// The 0x12 0x20 prefix is the multihash header for SHA-256, 32 bytes.
pub fn ipfs_content_id(data: &[u8]) -> ContentId {
    let digest = sha256_digest(data);
    let mut multihash = Vec::with_capacity(34);
    multihash.push(0x12); // SHA-256 function code
    multihash.push(0x20); // 32-byte digest length
    multihash.extend_from_slice(&digest);
    ContentId {
        provider: "ipfs".to_string(),
        hash: format!("Qm{}", bs58::encode(&multihash).into_string()),
    }
}

/// Generate a Filecoin-style content ID.
///
/// Format: `bafy` + base58(SHA-256(data))
pub fn filecoin_content_id(data: &[u8]) -> ContentId {
    let digest = sha256_digest(data);
    ContentId {
        provider: "filecoin".to_string(),
        hash: format!("bafy{}", bs58::encode(digest).into_string()),
    }
}

/// Compute a content-addressed identifier from a canonical `EnvironmentalReport`.
///
/// The CID is derived from the deterministic `serde_json` serialization of the
/// report, making it verifiable by any consumer (mobile, backend, smart contracts)
/// that holds the same report. The hash format is IPFS CIDv0-style (SHA-256 with
/// multihash prefix).
///
/// # Errors
///
/// Returns `Err` if `serde_json::to_vec` fails (which should not happen for
/// well-formed reports).
///
/// # Alignment with core4 / backend
///
/// Using `serde_json` serialization as the canonical wire format mirrors the approach
/// used in `core4/native-chain` state-root computation and the backend's weather
/// ingestion pipeline. Any party with the report JSON can independently verify the CID.
pub fn observation_content_id(report: &EnvironmentalReport) -> Result<ContentId, String> {
    let bytes = serde_json::to_vec(report)
        .map_err(|e| format!("observation_content_id: serialization failed: {e}"))?;
    Ok(ipfs_content_id(&bytes))
}

/// Generate an Arweave-style content ID.
///
/// Format: base64url(SHA-256(data)) with no padding.
pub fn arweave_content_id(data: &[u8]) -> ContentId {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let digest = sha256_digest(data);
    ContentId {
        provider: "arweave".to_string(),
        hash: URL_SAFE_NO_PAD.encode(digest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_deterministic() {
        let a = sha256_digest(b"hello weather data");
        let b = sha256_digest(b"hello weather data");
        assert_eq!(a, b);
    }

    #[test]
    fn test_sha256_different_data() {
        let a = sha256_digest(b"data A");
        let b = sha256_digest(b"data B");
        assert_ne!(a, b);
    }

    #[test]
    fn test_ipfs_content_id_prefix() {
        let cid = ipfs_content_id(b"test");
        assert_eq!(cid.provider, "ipfs");
        assert!(cid.hash.starts_with("Qm"), "got: {}", cid.hash);
    }

    #[test]
    fn test_filecoin_content_id_prefix() {
        let cid = filecoin_content_id(b"test");
        assert_eq!(cid.provider, "filecoin");
        assert!(cid.hash.starts_with("bafy"), "got: {}", cid.hash);
    }

    #[test]
    fn test_arweave_content_id_format() {
        let cid = arweave_content_id(b"test");
        assert_eq!(cid.provider, "arweave");
        // base64url of a 32-byte digest is 43 chars (no padding)
        assert_eq!(cid.hash.len(), 43);
    }

    #[test]
    fn test_ipfs_content_id_deterministic() {
        let a = ipfs_content_id(b"same data");
        let b = ipfs_content_id(b"same data");
        assert_eq!(a, b);
    }

    // ---- observation_content_id tests ----

    fn sample_report() -> crate::types::EnvironmentalReport {
        use crate::types::{
            EnvironmentalDomain, GeoLocation, ReportPayload, WeatherPayload,
        };
        crate::types::EnvironmentalReport {
            report_id: "hash-test-001".to_string(),
            domain: EnvironmentalDomain::Weather,
            station_id: "stn-hash-1".to_string(),
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
    fn test_observation_content_id_is_ipfs_prefixed() {
        let cid = observation_content_id(&sample_report()).unwrap();
        assert_eq!(cid.provider, "ipfs");
        assert!(cid.hash.starts_with("Qm"), "got: {}", cid.hash);
    }

    #[test]
    fn test_observation_content_id_is_deterministic() {
        let report = sample_report();
        let a = observation_content_id(&report).unwrap();
        let b = observation_content_id(&report).unwrap();
        assert_eq!(a, b, "same report must always produce the same CID");
    }

    #[test]
    fn test_observation_content_id_differs_for_different_reports() {
        let report_a = sample_report();
        let mut report_b = sample_report();
        report_b.report_id = "hash-test-002".to_string();
        let cid_a = observation_content_id(&report_a).unwrap();
        let cid_b = observation_content_id(&report_b).unwrap();
        assert_ne!(cid_a, cid_b, "different reports must produce different CIDs");
    }

    #[test]
    fn test_observation_content_id_changes_when_payload_changes() {
        use crate::types::ReportPayload;
        let report_a = sample_report();
        let mut report_b = sample_report();
        if let ReportPayload::Weather(ref mut w) = report_b.payload {
            w.temperature_c = Some(99.9);
        }
        let cid_a = observation_content_id(&report_a).unwrap();
        let cid_b = observation_content_id(&report_b).unwrap();
        assert_ne!(cid_a, cid_b, "payload change must change the CID");
    }
}
