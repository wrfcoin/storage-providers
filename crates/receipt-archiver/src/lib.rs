//! Receipt archiver for forecast verification receipts.
//!
//! Accepts forecast verification receipts, serializes them to canonical JSON,
//! and stores them via the `StorageProvider` trait with CID anchoring.
//!
//! # Example
//!
//! ```ignore
//! use receipt_archiver::{ForecastVerificationReceipt, archive_receipt};
//! use storage_common::StorageProvider;
//!
//! let receipt = ForecastVerificationReceipt { /* ... */ };
//! let (cid, arweave_tx) = archive_receipt(&provider, &receipt).await?;
//! println!("Archived receipt at {}", cid);
//! ```

mod canonical;
mod receipt;
pub mod archiver;

pub use canonical::{canonical_json, CanonicalError};
pub use receipt::{ForecastVerificationReceipt, ForecastAccuracyMetrics};
pub use archiver::{archive_receipt, ArchiveError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_json_determinism() {
        let json = r#"{"b": 2, "a": 1}"#;
        let receipt: serde_json::Value = serde_json::from_str(json).unwrap();
        let canonical1 = canonical_json(&receipt).unwrap();
        let canonical2 = canonical_json(&receipt).unwrap();
        assert_eq!(canonical1, canonical2, "canonical JSON must be deterministic");
    }

    #[test]
    fn test_canonical_json_sorted_keys() {
        let receipt = serde_json::json!({
            "z": 3,
            "a": 1,
            "m": 2
        });
        let canonical = canonical_json(&receipt).unwrap();
        let canonical_str = String::from_utf8(canonical).unwrap();
        // Keys should appear in sorted order: a, m, z
        let a_pos = canonical_str.find("\"a\"").unwrap();
        let m_pos = canonical_str.find("\"m\"").unwrap();
        let z_pos = canonical_str.find("\"z\"").unwrap();
        assert!(a_pos < m_pos && m_pos < z_pos, "keys must be sorted: {}", canonical_str);
    }
}
