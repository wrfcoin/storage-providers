//! Canonical JSON serialization for deterministic hashing.
//!
//! Provides functions to serialize Rust values to canonical (deterministic) JSON
//! with sorted keys and no whitespace, ensuring the same data always produces
//! the same byte representation for CID computation.

use serde::Serialize;
use thiserror::Error;

/// Error type for canonical JSON operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("canonical JSON error: {0}")]
pub struct CanonicalError(pub String);

/// Serialize a value to canonical JSON with sorted keys.
///
/// The output is deterministic: the same value will always produce the same
/// byte sequence, which is essential for computing stable content identifiers
/// (CIDs). Keys are recursively sorted in lexicographic order, and no
/// unnecessary whitespace is included.
///
/// # Arguments
///
/// * `value` - A serde-serializable value
///
/// # Returns
///
/// A `Vec<u8>` containing the canonical JSON representation.
///
/// # Errors
///
/// Returns [`CanonicalError`] if serialization fails.
///
/// # Example
///
/// ```ignore
/// let receipt = ForecastVerificationReceipt { /* ... */ };
/// let canonical_bytes = canonical_json(&receipt)?;
/// let cid = ipfs_content_id(&canonical_bytes);
/// ```
pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    // Serialize to an intermediate JSON value to apply sorting.
    let json_value = serde_json::to_value(value)
        .map_err(|e| CanonicalError(format!("serialization failed: {}", e)))?;

    // Recursively sort keys and serialize to a compact string.
    let sorted_value = sort_json_keys(&json_value);
    let compact = serde_json::to_string(&sorted_value)
        .map_err(|e| CanonicalError(format!("compact serialization failed: {}", e)))?;

    Ok(compact.into_bytes())
}

/// Recursively sort all keys in a JSON value for canonical representation.
fn sort_json_keys(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            for (key, val) in entries {
                sorted.insert(key.clone(), sort_json_keys(val));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(sort_json_keys).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_json_empty_object() {
        let value = serde_json::json!({});
        let canonical = canonical_json(&value).unwrap();
        assert_eq!(canonical, b"{}");
    }

    #[test]
    fn test_canonical_json_simple_sorted() {
        let value = serde_json::json!({
            "z": 3,
            "a": 1,
            "m": 2
        });
        let canonical = canonical_json(&value).unwrap();
        let canonical_str = String::from_utf8(canonical).unwrap();

        // Keys appear in sorted order in the string representation.
        let a_idx = canonical_str.find("\"a\"").unwrap();
        let m_idx = canonical_str.find("\"m\"").unwrap();
        let z_idx = canonical_str.find("\"z\"").unwrap();

        assert!(a_idx < m_idx, "a should come before m in {}", canonical_str);
        assert!(m_idx < z_idx, "m should come before z in {}", canonical_str);
    }

    #[test]
    fn test_canonical_json_deterministic() {
        let json_str = r#"{"b": 2, "a": 1, "c": 3}"#;
        let value1: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let value2: serde_json::Value = serde_json::from_str(json_str).unwrap();

        let canonical1 = canonical_json(&value1).unwrap();
        let canonical2 = canonical_json(&value2).unwrap();

        assert_eq!(canonical1, canonical2, "identical inputs must produce identical canonical output");
    }

    #[test]
    fn test_canonical_json_nested() {
        let value = serde_json::json!({
            "outer": {
                "z_key": 1,
                "a_key": 2
            },
            "another": 42
        });
        let canonical = canonical_json(&value).unwrap();
        let canonical_str = String::from_utf8(canonical).unwrap();

        // Check that outer keys are sorted.
        let another_idx = canonical_str.find("\"another\"").unwrap();
        let outer_idx = canonical_str.find("\"outer\"").unwrap();
        assert!(another_idx < outer_idx, "another should come before outer");

        // Check that nested keys are sorted (z_key before a_key is wrong, a_key should come first).
        let a_key_idx = canonical_str.find("\"a_key\"").unwrap();
        let z_key_idx = canonical_str.find("\"z_key\"").unwrap();
        assert!(a_key_idx < z_key_idx, "a_key should come before z_key in nested object");
    }

    #[test]
    fn test_canonical_json_array() {
        let value = serde_json::json!([3, 1, 2]);
        let canonical = canonical_json(&value).unwrap();
        // Arrays preserve order, not sorted.
        assert_eq!(canonical, b"[3,1,2]");
    }

    #[test]
    fn test_canonical_json_no_whitespace() {
        let value = serde_json::json!({
            "key": "value"
        });
        let canonical = canonical_json(&value).unwrap();
        let canonical_str = String::from_utf8(canonical).unwrap();

        // No unnecessary whitespace.
        assert!(!canonical_str.contains(" "), "canonical JSON should have no whitespace");
    }

    #[test]
    fn test_canonical_json_false_value() {
        let value = serde_json::json!(false);
        let canonical = canonical_json(&value).unwrap();
        assert_eq!(canonical, b"false");
    }

    #[test]
    fn test_canonical_json_null_value() {
        let value = serde_json::json!(null);
        let canonical = canonical_json(&value).unwrap();
        assert_eq!(canonical, b"null");
    }
}
