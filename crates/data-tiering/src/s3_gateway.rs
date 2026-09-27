//! S3-compatible gateway for legacy system integration.
//!
//! Provides a minimal S3 API shim that maps S3 keys to content-addressed identifiers,
//! enabling integration with existing tools and workflows that speak S3 (boto3, aws-cli, etc.).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wrfcoin_storage_common::types::{ContentId, DataTier};

/// Mapping of S3 bucket names to storage tiers.
#[derive(Debug, Clone)]
pub struct BucketTierMapping {
    mapping: HashMap<String, DataTier>,
}

impl BucketTierMapping {
    /// Create a new bucket-to-tier mapping with default WRFCoin buckets.
    pub fn new() -> Self {
        let mut mapping = HashMap::new();
        mapping.insert("wrfcoin-hot".to_string(), DataTier::Hot);
        mapping.insert("wrfcoin-warm".to_string(), DataTier::Warm);
        mapping.insert("wrfcoin-cold".to_string(), DataTier::Cold);
        mapping.insert("wrfcoin-archive".to_string(), DataTier::Archive);

        Self { mapping }
    }

    /// Add a custom bucket-to-tier mapping.
    pub fn add_bucket(&mut self, bucket: String, tier: DataTier) {
        self.mapping.insert(bucket, tier);
    }

    /// Get the tier for a bucket.
    pub fn get_tier(&self, bucket: &str) -> Option<DataTier> {
        self.mapping.get(bucket).copied()
    }

    /// List all configured buckets.
    pub fn buckets(&self) -> Vec<String> {
        self.mapping.keys().cloned().collect()
    }
}

impl Default for BucketTierMapping {
    fn default() -> Self {
        Self::new()
    }
}

/// S3 object key mapping to content identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Key {
    /// Bucket name (e.g., "wrfcoin-hot").
    pub bucket: String,
    /// Object key path (e.g., "weather/2026-03-16/station-42.json").
    pub key: String,
    /// Optional content ID if key maps to a CID.
    pub content_id: Option<ContentId>,
}

impl S3Key {
    /// Create a new S3 key.
    pub fn new(bucket: String, key: String) -> Self {
        Self {
            bucket,
            key,
            content_id: None,
        }
    }

    /// Create an S3 key with an associated content ID.
    pub fn with_content_id(bucket: String, key: String, content_id: ContentId) -> Self {
        Self {
            bucket,
            key,
            content_id: Some(content_id),
        }
    }

    /// Parse an S3 key as a ContentId (format: "provider:hash").
    pub fn parse_as_content_id(&self) -> Option<ContentId> {
        if let Some(cid) = &self.content_id {
            return Some(cid.clone());
        }

        // Try to parse key directly as "provider:hash"
        let parts: Vec<&str> = self.key.split(':').collect();
        if parts.len() == 2 {
            Some(ContentId {
                provider: parts[0].to_string(),
                hash: parts[1].to_string(),
            })
        } else {
            None
        }
    }

    /// Get a human-readable path for the key.
    pub fn full_path(&self) -> String {
        format!("s3://{}/{}", self.bucket, self.key)
    }
}

/// S3 GetObject request.
#[derive(Debug, Clone)]
pub struct GetObjectRequest {
    pub bucket: String,
    pub key: String,
}

/// S3 GetObject response.
#[derive(Debug, Clone)]
pub struct GetObjectResponse {
    pub bucket: String,
    pub key: String,
    pub data: Vec<u8>,
    pub content_type: String,
    pub content_length: u64,
    pub etag: String,
}

/// S3 PutObject request.
#[derive(Debug, Clone)]
pub struct PutObjectRequest {
    pub bucket: String,
    pub key: String,
    pub data: Vec<u8>,
    pub content_type: String,
}

/// S3 PutObject response.
#[derive(Debug, Clone)]
pub struct PutObjectResponse {
    pub bucket: String,
    pub key: String,
    pub etag: String,
    pub content_id: ContentId,
}

/// S3 HeadObject request.
#[derive(Debug, Clone)]
pub struct HeadObjectRequest {
    pub bucket: String,
    pub key: String,
}

/// S3 HeadObject response.
#[derive(Debug, Clone)]
pub struct HeadObjectResponse {
    pub bucket: String,
    pub key: String,
    pub exists: bool,
    pub content_length: Option<u64>,
    pub etag: Option<String>,
}

/// S3 DeleteObject request.
#[derive(Debug, Clone)]
pub struct DeleteObjectRequest {
    pub bucket: String,
    pub key: String,
}

/// S3 DeleteObject response.
#[derive(Debug, Clone)]
pub struct DeleteObjectResponse {
    pub bucket: String,
    pub key: String,
    pub deleted: bool,
}

/// S3 ListObjectsV2 request.
#[derive(Debug, Clone)]
pub struct ListObjectsV2Request {
    pub bucket: String,
    pub prefix: Option<String>,
    pub max_keys: Option<usize>,
}

/// S3 object metadata in ListObjectsV2 response.
#[derive(Debug, Clone)]
pub struct S3ObjectMetadata {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: u64, // Unix timestamp
}

/// S3 ListObjectsV2 response.
#[derive(Debug, Clone)]
pub struct ListObjectsV2Response {
    pub bucket: String,
    pub prefix: Option<String>,
    pub contents: Vec<S3ObjectMetadata>,
    pub is_truncated: bool,
    pub next_continuation_token: Option<String>,
}

/// S3 Gateway error type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum S3GatewayError {
    /// Bucket not found or not configured.
    BucketNotFound { bucket: String },
    /// Object not found in storage.
    ObjectNotFound { bucket: String, key: String },
    /// Invalid content ID format.
    InvalidContentId { reason: String },
    /// Storage operation failed.
    StorageError { reason: String },
    /// Authentication failed.
    AuthenticationError { reason: String },
    /// Invalid request.
    InvalidRequest { reason: String },
}

impl std::fmt::Display for S3GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            S3GatewayError::BucketNotFound { bucket } => write!(f, "Bucket not found: {bucket}"),
            S3GatewayError::ObjectNotFound { bucket, key } => {
                write!(f, "Object not found: s3://{bucket}/{key}")
            }
            S3GatewayError::InvalidContentId { reason } => {
                write!(f, "Invalid content ID: {reason}")
            }
            S3GatewayError::StorageError { reason } => write!(f, "Storage error: {reason}"),
            S3GatewayError::AuthenticationError { reason } => write!(f, "Auth error: {reason}"),
            S3GatewayError::InvalidRequest { reason } => write!(f, "Invalid request: {reason}"),
        }
    }
}

impl std::error::Error for S3GatewayError {}

/// S3-compatible gateway routing requests to storage providers.
pub struct S3Gateway {
    bucket_mapping: BucketTierMapping,
    /// In-memory object store for testing (maps S3 key to content).
    test_store: std::sync::RwLock<HashMap<String, Vec<u8>>>,
    /// Metrics: request counts per operation.
    metrics: std::sync::Mutex<HashMap<String, u64>>,
}

impl S3Gateway {
    /// Create a new S3 gateway with default bucket configuration.
    pub fn new() -> Self {
        Self {
            bucket_mapping: BucketTierMapping::new(),
            test_store: std::sync::RwLock::new(HashMap::new()),
            metrics: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Create a gateway with custom bucket mapping.
    pub fn with_buckets(mapping: BucketTierMapping) -> Self {
        Self {
            bucket_mapping: mapping,
            test_store: std::sync::RwLock::new(HashMap::new()),
            metrics: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Get the tier for a bucket.
    pub fn get_tier(&self, bucket: &str) -> Result<DataTier, S3GatewayError> {
        self.bucket_mapping
            .get_tier(bucket)
            .ok_or(S3GatewayError::BucketNotFound {
                bucket: bucket.to_string(),
            })
    }

    /// List all configured buckets.
    pub fn list_buckets(&self) -> Vec<String> {
        self.bucket_mapping.buckets()
    }

    /// Handle GetObject request.
    pub fn get_object(&self, req: &GetObjectRequest) -> Result<GetObjectResponse, S3GatewayError> {
        self.record_metric("GetObject");

        // Verify bucket exists
        self.get_tier(&req.bucket)?;

        let key_path = format!("{}/{}", req.bucket, req.key);

        // Retrieve from test store
        let store = self
            .test_store
            .read()
            .map_err(|_| S3GatewayError::StorageError {
                reason: "failed to acquire read lock".to_string(),
            })?;

        let data = store
            .get(&key_path)
            .ok_or(S3GatewayError::ObjectNotFound {
                bucket: req.bucket.clone(),
                key: req.key.clone(),
            })?
            .clone();

        drop(store);

        let etag = blake3::hash(&data).to_hex().to_string();

        Ok(GetObjectResponse {
            bucket: req.bucket.clone(),
            key: req.key.clone(),
            content_length: data.len() as u64,
            content_type: "application/octet-stream".to_string(),
            etag,
            data,
        })
    }

    /// Handle PutObject request.
    pub fn put_object(&self, req: &PutObjectRequest) -> Result<PutObjectResponse, S3GatewayError> {
        self.record_metric("PutObject");

        // Verify bucket exists
        self.get_tier(&req.bucket)?;

        let key_path = format!("{}/{}", req.bucket, req.key);

        // Store in test store
        let mut store = self
            .test_store
            .write()
            .map_err(|_| S3GatewayError::StorageError {
                reason: "failed to acquire write lock".to_string(),
            })?;

        store.insert(key_path.clone(), req.data.clone());

        // Generate ETag (blake3 hash of content)
        let etag = blake3::hash(&req.data).to_hex().to_string();

        // Generate or parse ContentId from key
        let content_id = S3Key::new(req.bucket.clone(), req.key.clone())
            .parse_as_content_id()
            .unwrap_or_else(|| {
                // Generate ContentId from content hash
                let hash = blake3::hash(&req.data).to_hex().to_string();
                ContentId {
                    provider: "s3".to_string(),
                    hash,
                }
            });

        Ok(PutObjectResponse {
            bucket: req.bucket.clone(),
            key: req.key.clone(),
            etag,
            content_id,
        })
    }

    /// Handle HeadObject request.
    pub fn head_object(
        &self,
        req: &HeadObjectRequest,
    ) -> Result<HeadObjectResponse, S3GatewayError> {
        self.record_metric("HeadObject");

        // Verify bucket exists
        self.get_tier(&req.bucket)?;

        let key_path = format!("{}/{}", req.bucket, req.key);

        let store = self
            .test_store
            .read()
            .map_err(|_| S3GatewayError::StorageError {
                reason: "failed to acquire read lock".to_string(),
            })?;

        let exists = store.contains_key(&key_path);
        let (content_length, etag) = if exists {
            let data = &store[&key_path];
            let etag = blake3::hash(data).to_hex().to_string();
            (Some(data.len() as u64), Some(etag))
        } else {
            (None, None)
        };

        Ok(HeadObjectResponse {
            bucket: req.bucket.clone(),
            key: req.key.clone(),
            exists,
            content_length,
            etag,
        })
    }

    /// Handle DeleteObject request.
    pub fn delete_object(
        &self,
        req: &DeleteObjectRequest,
    ) -> Result<DeleteObjectResponse, S3GatewayError> {
        self.record_metric("DeleteObject");

        // Verify bucket exists
        self.get_tier(&req.bucket)?;

        let key_path = format!("{}/{}", req.bucket, req.key);

        let mut store = self
            .test_store
            .write()
            .map_err(|_| S3GatewayError::StorageError {
                reason: "failed to acquire write lock".to_string(),
            })?;

        let deleted = store.remove(&key_path).is_some();

        Ok(DeleteObjectResponse {
            bucket: req.bucket.clone(),
            key: req.key.clone(),
            deleted,
        })
    }

    /// Handle ListObjectsV2 request.
    pub fn list_objects_v2(
        &self,
        req: &ListObjectsV2Request,
    ) -> Result<ListObjectsV2Response, S3GatewayError> {
        self.record_metric("ListObjectsV2");

        // Verify bucket exists
        self.get_tier(&req.bucket)?;

        let store = self
            .test_store
            .read()
            .map_err(|_| S3GatewayError::StorageError {
                reason: "failed to acquire read lock".to_string(),
            })?;

        let bucket_prefix = format!("{}/", req.bucket);
        let mut contents = Vec::new();

        for (key_path, data) in store.iter() {
            if !key_path.starts_with(&bucket_prefix) {
                continue;
            }

            let key = key_path[bucket_prefix.len()..].to_string();

            // Filter by prefix if provided
            if let Some(prefix) = &req.prefix {
                if !key.starts_with(prefix) {
                    continue;
                }
            }

            let etag = blake3::hash(data).to_hex().to_string();
            contents.push(S3ObjectMetadata {
                key,
                size: data.len() as u64,
                etag,
                last_modified: 0, // Placeholder
            });
        }

        // Apply max_keys limit
        let is_truncated = if let Some(max) = req.max_keys {
            contents.truncate(max);
            contents.len() >= max
        } else {
            false
        };

        Ok(ListObjectsV2Response {
            bucket: req.bucket.clone(),
            prefix: req.prefix.clone(),
            contents,
            is_truncated,
            next_continuation_token: None,
        })
    }

    /// Record a metric for request tracking.
    fn record_metric(&self, operation: &str) {
        if let Ok(mut metrics) = self.metrics.lock() {
            *metrics.entry(operation.to_string()).or_insert(0) += 1;
        }
    }

    /// Get metric counts.
    pub fn metrics(&self) -> HashMap<String, u64> {
        self.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Get metric for a specific operation.
    pub fn get_metric(&self, operation: &str) -> u64 {
        self.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(operation)
            .copied()
            .unwrap_or(0)
    }
}

impl Default for S3Gateway {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_tier_mapping_defaults() {
        let mapping = BucketTierMapping::new();
        assert_eq!(mapping.get_tier("wrfcoin-hot"), Some(DataTier::Hot));
        assert_eq!(mapping.get_tier("wrfcoin-warm"), Some(DataTier::Warm));
        assert_eq!(mapping.get_tier("wrfcoin-cold"), Some(DataTier::Cold));
        assert_eq!(mapping.get_tier("wrfcoin-archive"), Some(DataTier::Archive));
    }

    #[test]
    fn test_bucket_tier_mapping_custom() {
        let mut mapping = BucketTierMapping::new();
        mapping.add_bucket("custom-bucket".to_string(), DataTier::Hot);
        assert_eq!(mapping.get_tier("custom-bucket"), Some(DataTier::Hot));
    }

    #[test]
    fn test_s3_key_creation() {
        let key = S3Key::new("wrfcoin-hot".to_string(), "weather/data.json".to_string());
        assert_eq!(key.bucket, "wrfcoin-hot");
        assert_eq!(key.key, "weather/data.json");
        assert_eq!(key.full_path(), "s3://wrfcoin-hot/weather/data.json");
    }

    #[test]
    fn test_s3_key_parse_content_id() {
        let key = S3Key::new("wrfcoin-hot".to_string(), "ipfs:QmTest123".to_string());
        let cid = key.parse_as_content_id();
        assert!(cid.is_some());
        let cid = cid.unwrap();
        assert_eq!(cid.provider, "ipfs");
        assert_eq!(cid.hash, "QmTest123");
    }

    #[test]
    fn test_s3_key_with_content_id() {
        let cid = ContentId {
            provider: "filecoin".to_string(),
            hash: "bafy456".to_string(),
        };
        let key = S3Key::with_content_id(
            "wrfcoin-warm".to_string(),
            "forecast/model.bin".to_string(),
            cid.clone(),
        );
        assert_eq!(key.content_id, Some(cid));
    }

    #[test]
    fn test_s3_gateway_put_and_get() {
        let gateway = S3Gateway::new();
        let test_data = b"weather observation data";

        // Put object
        let put_req = PutObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "observations/2026-03-16.json".to_string(),
            data: test_data.to_vec(),
            content_type: "application/json".to_string(),
        };
        let put_resp = gateway.put_object(&put_req).unwrap();
        assert_eq!(put_resp.bucket, "wrfcoin-hot");
        assert!(!put_resp.etag.is_empty());

        // Get object
        let get_req = GetObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "observations/2026-03-16.json".to_string(),
        };
        let get_resp = gateway.get_object(&get_req).unwrap();
        assert_eq!(get_resp.data, test_data);
        assert_eq!(get_resp.content_length, test_data.len() as u64);
        assert_eq!(put_resp.etag, get_resp.etag);
    }

    #[test]
    fn test_s3_gateway_head_object() {
        let gateway = S3Gateway::new();

        // Object doesn't exist
        let head_req = HeadObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "nonexistent.json".to_string(),
        };
        let head_resp = gateway.head_object(&head_req).unwrap();
        assert!(!head_resp.exists);

        // Put object
        let put_req = PutObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "test.json".to_string(),
            data: b"test data".to_vec(),
            content_type: "application/json".to_string(),
        };
        gateway.put_object(&put_req).unwrap();

        // Head object
        let head_req = HeadObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "test.json".to_string(),
        };
        let head_resp = gateway.head_object(&head_req).unwrap();
        assert!(head_resp.exists);
        assert_eq!(head_resp.content_length, Some(9));
        assert!(head_resp.etag.is_some());
    }

    #[test]
    fn test_s3_gateway_delete_object() {
        let gateway = S3Gateway::new();

        // Put object
        let put_req = PutObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "to-delete.json".to_string(),
            data: b"temporary data".to_vec(),
            content_type: "application/json".to_string(),
        };
        gateway.put_object(&put_req).unwrap();

        // Delete object
        let del_req = DeleteObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "to-delete.json".to_string(),
        };
        let del_resp = gateway.delete_object(&del_req).unwrap();
        assert!(del_resp.deleted);

        // Verify deleted
        let head_req = HeadObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "to-delete.json".to_string(),
        };
        let head_resp = gateway.head_object(&head_req).unwrap();
        assert!(!head_resp.exists);
    }

    #[test]
    fn test_s3_gateway_list_objects_v2() {
        let gateway = S3Gateway::new();

        // Put multiple objects
        for i in 0..5 {
            let put_req = PutObjectRequest {
                bucket: "wrfcoin-hot".to_string(),
                key: format!("data/file{i}.json"),
                data: format!("content {i}").into_bytes(),
                content_type: "application/json".to_string(),
            };
            gateway.put_object(&put_req).unwrap();
        }

        // List all
        let list_req = ListObjectsV2Request {
            bucket: "wrfcoin-hot".to_string(),
            prefix: None,
            max_keys: None,
        };
        let list_resp = gateway.list_objects_v2(&list_req).unwrap();
        assert_eq!(list_resp.contents.len(), 5);

        // List with prefix
        let list_req = ListObjectsV2Request {
            bucket: "wrfcoin-hot".to_string(),
            prefix: Some("data/".to_string()),
            max_keys: None,
        };
        let list_resp = gateway.list_objects_v2(&list_req).unwrap();
        assert_eq!(list_resp.contents.len(), 5);

        // List with max_keys
        let list_req = ListObjectsV2Request {
            bucket: "wrfcoin-hot".to_string(),
            prefix: None,
            max_keys: Some(2),
        };
        let list_resp = gateway.list_objects_v2(&list_req).unwrap();
        assert_eq!(list_resp.contents.len(), 2);
        assert!(list_resp.is_truncated);
    }

    #[test]
    fn test_s3_gateway_invalid_bucket() {
        let gateway = S3Gateway::new();

        let get_req = GetObjectRequest {
            bucket: "nonexistent-bucket".to_string(),
            key: "data.json".to_string(),
        };
        let result = gateway.get_object(&get_req);
        assert!(result.is_err());

        if let Err(S3GatewayError::BucketNotFound { bucket }) = result {
            assert_eq!(bucket, "nonexistent-bucket");
        } else {
            panic!("expected BucketNotFound error");
        }
    }

    #[test]
    fn test_s3_gateway_metrics() {
        let gateway = S3Gateway::new();

        // Perform operations
        let put_req = PutObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "test.json".to_string(),
            data: b"test".to_vec(),
            content_type: "application/json".to_string(),
        };
        gateway.put_object(&put_req).unwrap();

        let get_req = GetObjectRequest {
            bucket: "wrfcoin-hot".to_string(),
            key: "test.json".to_string(),
        };
        gateway.get_object(&get_req).unwrap();
        gateway.get_object(&get_req).unwrap();

        // Check metrics
        assert_eq!(gateway.get_metric("PutObject"), 1);
        assert_eq!(gateway.get_metric("GetObject"), 2);
    }

    #[test]
    fn test_s3_gateway_multiple_buckets() {
        let gateway = S3Gateway::new();

        // Put to different buckets
        let buckets = vec!["wrfcoin-hot", "wrfcoin-warm", "wrfcoin-cold"];
        for bucket in buckets {
            let put_req = PutObjectRequest {
                bucket: bucket.to_string(),
                key: "data.json".to_string(),
                data: b"test data".to_vec(),
                content_type: "application/json".to_string(),
            };
            let result = gateway.put_object(&put_req);
            assert!(result.is_ok());
        }

        // List buckets
        let buckets = gateway.list_buckets();
        assert!(buckets.contains(&"wrfcoin-hot".to_string()));
        assert!(buckets.contains(&"wrfcoin-warm".to_string()));
    }

    #[test]
    fn test_s3_gateway_tier_lookup() {
        let gateway = S3Gateway::new();

        assert_eq!(gateway.get_tier("wrfcoin-hot").unwrap(), DataTier::Hot);
        assert_eq!(gateway.get_tier("wrfcoin-warm").unwrap(), DataTier::Warm);
        assert!(gateway.get_tier("invalid-bucket").is_err());
    }
}
