//! Blake3-based integrity verification for stored content.

/// Verifier that uses blake3 hashes for content integrity checking.
pub struct IntegrityVerifier;

impl IntegrityVerifier {
    /// Compute blake3 hash of data, returning a hex string.
    pub fn hash(data: &[u8]) -> String {
        blake3::hash(data).to_hex().to_string()
    }

    /// Verify data matches the expected blake3 hex hash.
    pub fn verify(data: &[u8], expected_hash: &str) -> bool {
        Self::hash(data) == expected_hash
    }

    /// Compute hash and return both hash and original data (for store-and-verify pattern).
    pub fn hash_and_wrap(data: &[u8]) -> (String, &[u8]) {
        (Self::hash(data), data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_deterministic() {
        let a = IntegrityVerifier::hash(b"weather data 123");
        let b = IntegrityVerifier::hash(b"weather data 123");
        assert_eq!(a, b);
    }

    #[test]
    fn test_hash_different_data() {
        let a = IntegrityVerifier::hash(b"data A");
        let b = IntegrityVerifier::hash(b"data B");
        assert_ne!(a, b);
    }

    #[test]
    fn test_verify_correct() {
        let data = b"test payload";
        let hash = IntegrityVerifier::hash(data);
        assert!(IntegrityVerifier::verify(data, &hash));
    }

    #[test]
    fn test_verify_incorrect() {
        let data = b"test payload";
        assert!(!IntegrityVerifier::verify(data, "badhash"));
    }

    #[test]
    fn test_hash_and_wrap() {
        let data = b"wrap test";
        let (hash, wrapped) = IntegrityVerifier::hash_and_wrap(data);
        assert_eq!(wrapped, data);
        assert!(IntegrityVerifier::verify(wrapped, &hash));
    }

    #[test]
    fn test_hash_is_hex_string() {
        let hash = IntegrityVerifier::hash(b"hex test");
        // blake3 produces 64-char hex strings (32 bytes)
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
