use sha2::{Digest, Sha256};

use crate::types::ContentId;

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
}
