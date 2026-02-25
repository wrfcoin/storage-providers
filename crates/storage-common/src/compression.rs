use std::io::Read;

use flate2::read::{GzDecoder, GzEncoder};
use flate2::Compression;

use crate::error::StorageError;

/// Compress data using gzip.
pub fn compress(data: &[u8]) -> Result<Vec<u8>, StorageError> {
    let mut encoder = GzEncoder::new(data, Compression::default());
    let mut compressed = Vec::new();
    encoder
        .read_to_end(&mut compressed)
        .map_err(|e| StorageError::InvalidData(format!("compression failed: {e}")))?;
    Ok(compressed)
}

/// Decompress gzip data.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, StorageError> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| StorageError::InvalidData(format!("decompression failed: {e}")))?;
    Ok(decompressed)
}

/// Check if data has gzip magic bytes (0x1F 0x8B).
pub fn is_compressed(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x1F && data[1] == 0x8B
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_roundtrip() {
        let original = b"hello world weather data 2026";
        let compressed = compress(original).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_weather_data() {
        // Repetitive weather data compresses well.
        let weather = r#"{"temp":22.5,"humidity":65,"pressure":1013.25,"wind":3.2}"#;
        let repeated: Vec<u8> = weather.as_bytes().repeat(100);
        let compressed = compress(&repeated).unwrap();
        // Should achieve > 50% compression on repetitive JSON.
        assert!(
            compressed.len() < repeated.len() / 2,
            "compressed {} vs original {}",
            compressed.len(),
            repeated.len()
        );
    }

    #[test]
    fn test_decompress_invalid_data() {
        let result = decompress(b"not gzip data");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_compressed_true() {
        let data = compress(b"test").unwrap();
        assert!(is_compressed(&data));
    }

    #[test]
    fn test_is_compressed_false() {
        assert!(!is_compressed(b"plain text"));
        assert!(!is_compressed(b""));
        assert!(!is_compressed(b"x"));
    }
}
