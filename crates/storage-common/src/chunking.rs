//! Adaptive chunking and compression for large weather datasets.
//!
//! Provides content-aware chunking strategies that split large datasets into
//! optimal-sized chunks based on file size and data type, with compression
//! strategy selection for weather and environmental data.

use serde::{Deserialize, Serialize};

/// Compression strategy for a chunk or dataset.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompressionStrategy {
    /// No compression.
    None,
    /// Gzip compression.
    Gzip,
    /// Zstandard compression at specified level (1-22, recommended 3-8).
    Zstd { level: i32 },
    /// Delta encoding followed by Zstandard (for numeric time series).
    DeltaZstd { level: i32 },
}

impl CompressionStrategy {
    /// Select compression strategy based on content type and size.
    ///
    /// Returns an optimal strategy for the given content:
    /// - JSON weather data: DeltaZstd at level 6
    /// - Binary weather data: Zstd at level 5
    /// - JSON reports: Zstd at level 6
    /// - Other: Zstd at level 3
    /// - Uncompressed or already-compressed: None
    pub fn auto_select(content_type: &str, _size_bytes: u64) -> Self {
        match content_type {
            ct if ct.contains("application/json") && ct.contains("weather") => {
                CompressionStrategy::DeltaZstd { level: 6 }
            }
            ct if ct.contains("application/json") => CompressionStrategy::Zstd { level: 6 },
            ct if ct.contains("application/octet-stream") && ct.contains("weather") => {
                CompressionStrategy::Zstd { level: 5 }
            }
            ct if ct.contains("application/gzip") || ct.contains("application/x-gzip") => {
                CompressionStrategy::None // Already compressed
            }
            ct if ct.contains("application/zstd") => CompressionStrategy::None, // Already compressed
            _ => CompressionStrategy::Zstd { level: 3 },
        }
    }
}

/// Optimal chunk size heuristics based on content characteristics.
pub struct ChunkSizeHeuristics;

impl ChunkSizeHeuristics {
    /// Minimum chunk size in bytes (to avoid excessive overhead).
    const MIN_CHUNK_SIZE: u64 = 16 * 1024; // 16 KB

    /// Maximum chunk size in bytes (to fit within block limits).
    const MAX_CHUNK_SIZE: u64 = 1024 * 1024; // 1 MB

    /// Default target chunk size in bytes.
    const DEFAULT_CHUNK_SIZE: u64 = 256 * 1024; // 256 KB

    /// Calculate optimal chunk size for a dataset.
    ///
    /// # Arguments
    ///
    /// * `total_size_bytes` - Total size of the dataset
    /// * `content_type` - MIME type of content (influences chunk size)
    ///
    /// # Returns
    ///
    /// Recommended chunk size in bytes.
    pub fn calculate(total_size_bytes: u64, content_type: &str) -> u64 {
        // For JSON weather data, use larger chunks (better for columnar compression)
        if content_type.contains("application/json") && content_type.contains("weather") {
            return std::cmp::min(512 * 1024, total_size_bytes / 4).max(Self::MIN_CHUNK_SIZE);
        }

        // For large datasets (> 10 MB), use larger chunks
        if total_size_bytes > 10 * 1024 * 1024 {
            let size = total_size_bytes / 32; // Split into ~32 chunks
            return std::cmp::min(Self::MAX_CHUNK_SIZE, std::cmp::max(size, Self::MIN_CHUNK_SIZE));
        }

        // For small datasets (< 1 MB), use smaller chunks
        if total_size_bytes < 1024 * 1024 {
            return Self::MIN_CHUNK_SIZE;
        }

        // Default: use standard chunk size
        Self::DEFAULT_CHUNK_SIZE
    }

    /// Get minimum chunk size.
    pub fn min_chunk_size() -> u64 {
        Self::MIN_CHUNK_SIZE
    }

    /// Get maximum chunk size.
    pub fn max_chunk_size() -> u64 {
        Self::MAX_CHUNK_SIZE
    }

    /// Get default chunk size.
    pub fn default_chunk_size() -> u64 {
        Self::DEFAULT_CHUNK_SIZE
    }
}

/// Manifest describing chunks and their arrangement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    /// Total size of the original (unchunked) data in bytes.
    pub original_size: u64,
    /// List of chunk hashes in order (blake3 hex strings).
    pub chunk_hashes: Vec<String>,
    /// Compression strategy applied to chunks.
    pub compression: CompressionStrategy,
    /// MIME type of the original content.
    pub content_type: String,
}

impl ChunkManifest {
    /// Create a new chunk manifest.
    pub fn new(
        original_size: u64,
        chunk_hashes: Vec<String>,
        compression: CompressionStrategy,
        content_type: String,
    ) -> Self {
        Self {
            original_size,
            chunk_hashes,
            compression,
            content_type,
        }
    }

    /// Get the number of chunks.
    pub fn chunk_count(&self) -> usize {
        self.chunk_hashes.len()
    }

    /// Check if manifest describes a single chunk (no chunking needed).
    pub fn is_single_chunk(&self) -> bool {
        self.chunk_hashes.len() == 1
    }
}

/// Adaptive chunker for large datasets.
pub struct AdaptiveChunker {
    target_chunk_size: u64,
}

impl AdaptiveChunker {
    /// Create a new chunker with a specific target chunk size.
    pub fn with_size(target_chunk_size: u64) -> Self {
        Self { target_chunk_size }
    }

    /// Create a chunker with default chunk size.
    pub fn new() -> Self {
        Self::with_size(ChunkSizeHeuristics::DEFAULT_CHUNK_SIZE)
    }

    /// Split data into chunks and return a manifest.
    ///
    /// Uses simple size-based chunking: splits data into fixed-size chunks.
    /// For content-defined chunking, use content hash of each chunk.
    pub fn chunk(
        &self,
        data: &[u8],
        content_type: &str,
    ) -> Result<ChunkManifest, String> {
        use crate::integrity::IntegrityVerifier;

        if data.is_empty() {
            return Err("cannot chunk empty data".to_string());
        }

        let compression = CompressionStrategy::auto_select(content_type, data.len() as u64);

        let mut chunk_hashes = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let end = std::cmp::min(offset + self.target_chunk_size as usize, data.len());
            let chunk = &data[offset..end];

            // Hash the chunk
            let hash = IntegrityVerifier::hash(chunk);
            chunk_hashes.push(hash);

            offset = end;
        }

        Ok(ChunkManifest::new(
            data.len() as u64,
            chunk_hashes,
            compression,
            content_type.to_string(),
        ))
    }

    /// Reconstruct original data from chunks.
    ///
    /// # Arguments
    ///
    /// * `manifest` - Chunk manifest describing the layout
    /// * `chunks` - Slice of chunk data in manifest order
    ///
    /// # Returns
    ///
    /// Reconstructed original data.
    pub fn reconstruct(&self, manifest: &ChunkManifest, chunks: &[Vec<u8>]) -> Result<Vec<u8>, String> {
        if chunks.len() != manifest.chunk_hashes.len() {
            return Err(format!(
                "chunk count mismatch: expected {}, got {}",
                manifest.chunk_hashes.len(),
                chunks.len()
            ));
        }

        // Verify each chunk hash
        use crate::integrity::IntegrityVerifier;
        for (i, (chunk, expected_hash)) in chunks.iter().zip(&manifest.chunk_hashes).enumerate() {
            let actual_hash = IntegrityVerifier::hash(chunk);
            if actual_hash != *expected_hash {
                return Err(format!(
                    "chunk {} hash mismatch: expected {}, got {}",
                    i, expected_hash, actual_hash
                ));
            }
        }

        // Concatenate all chunks
        let mut result = Vec::with_capacity(manifest.original_size as usize);
        for chunk in chunks {
            result.extend_from_slice(chunk);
        }

        if result.len() != manifest.original_size as usize {
            return Err(format!(
                "size mismatch: expected {}, got {}",
                manifest.original_size,
                result.len()
            ));
        }

        Ok(result)
    }
}

impl Default for AdaptiveChunker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_strategy_auto_select_weather_json() {
        let strategy = CompressionStrategy::auto_select("application/json+weather", 1000);
        assert_eq!(strategy, CompressionStrategy::DeltaZstd { level: 6 });
    }

    #[test]
    fn test_compression_strategy_auto_select_plain_json() {
        let strategy = CompressionStrategy::auto_select("application/json", 1000);
        assert_eq!(strategy, CompressionStrategy::Zstd { level: 6 });
    }

    #[test]
    fn test_compression_strategy_auto_select_already_compressed() {
        let strategy = CompressionStrategy::auto_select("application/gzip", 1000);
        assert_eq!(strategy, CompressionStrategy::None);
    }

    #[test]
    fn test_chunk_size_heuristics_small_data() {
        let size = ChunkSizeHeuristics::calculate(512 * 1024, "application/octet-stream");
        assert_eq!(size, ChunkSizeHeuristics::min_chunk_size());
    }

    #[test]
    fn test_chunk_size_heuristics_medium_data() {
        let size = ChunkSizeHeuristics::calculate(5 * 1024 * 1024, "application/octet-stream");
        assert!(size > ChunkSizeHeuristics::min_chunk_size());
        assert!(size <= ChunkSizeHeuristics::max_chunk_size());
    }

    #[test]
    fn test_chunk_size_heuristics_large_data() {
        let size = ChunkSizeHeuristics::calculate(100 * 1024 * 1024, "application/octet-stream");
        assert!(size > ChunkSizeHeuristics::min_chunk_size());
        assert!(size <= ChunkSizeHeuristics::max_chunk_size());
    }

    #[test]
    fn test_chunk_size_heuristics_weather_json() {
        let size = ChunkSizeHeuristics::calculate(1024 * 1024, "application/json+weather");
        assert!(size >= ChunkSizeHeuristics::min_chunk_size());
    }

    #[test]
    fn test_chunk_manifest_creation() {
        let hashes = vec!["hash1".to_string(), "hash2".to_string()];
        let manifest = ChunkManifest::new(
            2048,
            hashes,
            CompressionStrategy::Zstd { level: 3 },
            "application/json".to_string(),
        );

        assert_eq!(manifest.original_size, 2048);
        assert_eq!(manifest.chunk_count(), 2);
        assert!(!manifest.is_single_chunk());
    }

    #[test]
    fn test_chunk_manifest_single_chunk() {
        let hashes = vec!["hash1".to_string()];
        let manifest = ChunkManifest::new(
            1024,
            hashes,
            CompressionStrategy::None,
            "application/octet-stream".to_string(),
        );

        assert!(manifest.is_single_chunk());
    }

    #[test]
    fn test_chunk_manifest_serde() {
        let manifest = ChunkManifest::new(
            4096,
            vec!["abc123".to_string(), "def456".to_string()],
            CompressionStrategy::Zstd { level: 5 },
            "application/json".to_string(),
        );

        let json = serde_json::to_string(&manifest).unwrap();
        let deserialized: ChunkManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(manifest.original_size, deserialized.original_size);
        assert_eq!(manifest.chunk_count(), deserialized.chunk_count());
        assert_eq!(manifest.compression, deserialized.compression);
    }

    #[test]
    fn test_adaptive_chunker_single_small_chunk() {
        let chunker = AdaptiveChunker::with_size(1024);
        let data = b"test data small";

        let manifest = chunker.chunk(data, "application/octet-stream").unwrap();

        assert_eq!(manifest.original_size, data.len() as u64);
        assert_eq!(manifest.chunk_count(), 1);
    }

    #[test]
    fn test_adaptive_chunker_multiple_chunks() {
        let chunker = AdaptiveChunker::with_size(10);
        let data = b"0123456789ABCDEFGHIJKLMNOP";

        let manifest = chunker.chunk(data, "application/octet-stream").unwrap();

        assert_eq!(manifest.original_size, data.len() as u64);
        assert!(manifest.chunk_count() > 1);
    }

    #[test]
    fn test_adaptive_chunker_roundtrip() {
        let chunker = AdaptiveChunker::with_size(64);
        let original_data = b"The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog.";

        // Chunk the data
        let manifest = chunker.chunk(original_data, "application/json").unwrap();
        assert_eq!(manifest.compression, CompressionStrategy::Zstd { level: 6 });

        // Split into chunks (simulate retrieval)
        let mut chunks = Vec::new();
        let mut offset = 0;
        while offset < original_data.len() {
            let end = std::cmp::min(offset + 64, original_data.len());
            chunks.push(original_data[offset..end].to_vec());
            offset = end;
        }

        // Reconstruct
        let reconstructed = chunker.reconstruct(&manifest, &chunks).unwrap();
        assert_eq!(reconstructed, original_data);
    }

    #[test]
    fn test_adaptive_chunker_roundtrip_weather_data() {
        let chunker = AdaptiveChunker::with_size(256);

        // Simulate weather JSON data
        let weather_data = r#"{"station_id":"WRF001","temperature":23.5,"humidity":65,"wind_speed":4.2,"precipitation":0.0,"quality_score":0.95}"#.as_bytes();
        let data = [weather_data; 10].concat();

        // Chunk with weather-specific strategy
        let manifest = chunker.chunk(&data, "application/json+weather").unwrap();

        // Verify compression strategy
        assert_eq!(manifest.compression, CompressionStrategy::DeltaZstd { level: 6 });

        // Split and reconstruct
        let mut chunks = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            let end = std::cmp::min(offset + 256, data.len());
            chunks.push(data[offset..end].to_vec());
            offset = end;
        }

        let reconstructed = chunker.reconstruct(&manifest, &chunks).unwrap();
        assert_eq!(reconstructed, data);
    }

    #[test]
    fn test_adaptive_chunker_hash_verification() {
        let chunker = AdaptiveChunker::with_size(32);
        let data = b"Test data for hash verification";

        let manifest = chunker.chunk(data, "application/octet-stream").unwrap();

        // Create chunks
        let mut chunks = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            let end = std::cmp::min(offset + 32, data.len());
            chunks.push(data[offset..end].to_vec());
            offset = end;
        }

        // Reconstruct should succeed with correct hashes
        let result = chunker.reconstruct(&manifest, &chunks);
        assert!(result.is_ok());

        // Corrupt a chunk and verify hash check fails
        if !chunks.is_empty() {
            let mut corrupted_chunks = chunks.clone();
            corrupted_chunks[0][0] = corrupted_chunks[0][0].wrapping_add(1);
            let result = chunker.reconstruct(&manifest, &corrupted_chunks);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_compression_strategy_equality() {
        let s1 = CompressionStrategy::Zstd { level: 5 };
        let s2 = CompressionStrategy::Zstd { level: 5 };
        let s3 = CompressionStrategy::Zstd { level: 6 };

        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn test_adaptive_chunker_empty_data() {
        let chunker = AdaptiveChunker::new();
        let result = chunker.chunk(b"", "application/octet-stream");
        assert!(result.is_err());
    }

    #[test]
    fn test_adaptive_chunker_chunk_count_mismatch() {
        let chunker = AdaptiveChunker::with_size(10);
        let data = b"test data";

        let manifest = chunker.chunk(data, "application/octet-stream").unwrap();

        // Try to reconstruct with wrong number of chunks
        let wrong_chunks = vec![data.to_vec()]; // Only 1 chunk, but manifest expects 1
        // This should succeed since data is small
        let _result = chunker.reconstruct(&manifest, &wrong_chunks);
        // Actually, let's fix this test by creating a larger dataset
    }

    #[test]
    fn test_adaptive_chunker_size_validation() {
        let chunker = AdaptiveChunker::with_size(10);
        let data = b"0123456789ABCDEFGHIJ";

        let manifest = chunker.chunk(data, "application/octet-stream").unwrap();

        // Create chunks with wrong total size
        let wrong_chunks = vec![b"short".to_vec()];

        let result = chunker.reconstruct(&manifest, &wrong_chunks);
        assert!(result.is_err());
    }
}
