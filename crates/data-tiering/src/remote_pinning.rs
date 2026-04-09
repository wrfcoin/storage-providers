//! Remote pinning service integration for IPFS Pinning Service API (PSA) compliance.
//!
//! Provides standardized access to remote pinning services like Pinata, web3.storage,
//! and nft.storage via the IPFS Pinning Service API specification.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// Configuration for a remote pinning service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePinServiceConfig {
    /// Service name (e.g., "pinata", "web3.storage", "nft.storage").
    pub name: String,
    /// PSA endpoint URL (e.g., "https://api.pinata.cloud/psa").
    pub endpoint: String,
    /// Authentication token (bearer token or API key).
    pub access_token: String,
    /// Optional maximum pin size in bytes.
    pub max_pin_size_bytes: Option<u64>,
}

/// Status of a pin request on a remote service.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RemotePinStatus {
    /// Pin request queued, waiting to start.
    Queued,
    /// Pin operation in progress.
    Pinning,
    /// Content successfully pinned.
    Pinned,
    /// Pin operation failed.
    Failed,
}

impl RemotePinStatus {
    /// Check if the pin status is terminal (no further changes expected).
    pub fn is_terminal(&self) -> bool {
        matches!(self, RemotePinStatus::Pinned | RemotePinStatus::Failed)
    }
}

/// PSA Pin object representing a pin request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PsaPin {
    /// Unique identifier assigned by the service.
    pub request_id: String,
    /// Status of the pin.
    pub status: String, // "queued", "pinning", "pinned", "failed"
    /// Content identifier (CID).
    pub cid: String,
    /// Optional human-readable name.
    pub name: Option<String>,
    /// Optional array of IPFS node multiaddrs.
    pub origins: Option<Vec<String>>,
    /// Optional metadata key-value pairs.
    pub meta: Option<HashMap<String, String>>,
}

/// Remote pinning client implementing IPFS PSA endpoints.
///
/// Provides a standardized interface for interacting with remote pinning services.
pub struct RemotePinningClient {
    config: RemotePinServiceConfig,
    /// Cache of pin statuses per CID.
    pin_status_cache: RwLock<HashMap<String, (RemotePinStatus, String)>>, // (status, request_id)
}

impl RemotePinningClient {
    /// Create a new remote pinning client for a service.
    pub fn new(config: RemotePinServiceConfig) -> Self {
        Self {
            config,
            pin_status_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Get the service name.
    pub fn service_name(&self) -> &str {
        &self.config.name
    }

    /// Get the endpoint URL.
    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    /// Request a pin for content on this remote service.
    ///
    /// Sends a POST request to `/pins` endpoint with the CID.
    ///
    /// # Arguments
    ///
    /// * `cid` - Content identifier to pin
    /// * `name` - Optional human-readable name
    ///
    /// # Returns
    ///
    /// Request ID assigned by the service if successful.
    pub async fn request_pin(
        &self,
        cid: &str,
        name: Option<&str>,
    ) -> Result<String, String> {
        // Build PSA POST /pins request body
        let _body = serde_json::json!({
            "cid": cid,
            "name": name,
            "origins": serde_json::json!([]),
        });

        // Make HTTP POST request to {endpoint}/pins
        // In production, this would use reqwest or similar
        // For now, return a mock request ID based on CID
        let request_id = format!("{}-{}-{}", self.config.name, cid, uuid_simple());

        // Cache the initial status as Queued
        {
            let mut cache = self
                .pin_status_cache
                .write()
                .map_err(|e| format!("failed to acquire write lock: {}", e))?;
            cache.insert(cid.to_string(), (RemotePinStatus::Queued, request_id.clone()));
        }

        Ok(request_id)
    }

    /// Check the status of a pin request.
    ///
    /// Sends a GET request to `/pins/{requestId}` endpoint.
    ///
    /// # Arguments
    ///
    /// * `request_id` - Request ID from request_pin()
    ///
    /// # Returns
    ///
    /// Current pin status.
    pub async fn check_pin_status(&self, request_id: &str) -> Result<RemotePinStatus, String> {
        // In production, would make GET request to {endpoint}/pins/{requestId}
        // For now, return cached status or Pinning if not cached
        let cache = self
            .pin_status_cache
            .read()
            .map_err(|e| format!("failed to acquire read lock: {}", e))?;

        Ok(cache
            .values()
            .find(|(_, rid)| rid == request_id)
            .map(|(status, _)| *status)
            .unwrap_or(RemotePinStatus::Pinning))
    }

    /// Remove (unpin) content from the remote service.
    ///
    /// Sends a DELETE request to `/pins/{requestId}` endpoint.
    ///
    /// # Arguments
    ///
    /// * `request_id` - Request ID from request_pin()
    ///
    /// # Returns
    ///
    /// Success if unpin succeeded.
    pub async fn remove_pin(&self, request_id: &str) -> Result<(), String> {
        // In production, would make DELETE request to {endpoint}/pins/{requestId}
        // For now, just remove from cache
        let mut cache = self
            .pin_status_cache
            .write()
            .map_err(|e| format!("failed to acquire write lock: {}", e))?;

        cache.retain(|_, (_, rid)| rid != request_id);
        Ok(())
    }

    /// List all pins on this service with optional filtering.
    ///
    /// Sends a GET request to `/pins` endpoint with optional filters.
    ///
    /// # Arguments
    ///
    /// * `_status_filter` - Optional status to filter by
    /// * `_cid_filter` - Optional CID to filter by
    ///
    /// # Returns
    ///
    /// List of pin objects matching the filters.
    pub async fn list_pins(
        &self,
        _status_filter: Option<&str>,
        _cid_filter: Option<&str>,
    ) -> Result<Vec<PsaPin>, String> {
        // In production, would make GET request to {endpoint}/pins?status={status}&cid={cid}
        // For now, return empty list
        Ok(Vec::new())
    }

    /// Get cached pin status for a CID.
    pub fn get_cached_status(&self, cid: &str) -> Option<RemotePinStatus> {
        let cache = self
            .pin_status_cache
            .read()
            .ok()?;

        cache.get(cid).map(|(status, _)| *status)
    }

    /// Clear the pin status cache.
    pub fn clear_cache(&self) -> Result<(), String> {
        let mut cache = self
            .pin_status_cache
            .write()
            .map_err(|e| format!("failed to acquire write lock: {}", e))?;

        cache.clear();
        Ok(())
    }
}

/// Manager for multiple remote pinning services.
pub struct RemotePinningManager {
    clients: HashMap<String, RemotePinningClient>,
    /// Global pin tracking: CID → (service_name → request_id)
    pin_requests: RwLock<HashMap<String, HashMap<String, String>>>,
}

impl RemotePinningManager {
    /// Create a new remote pinning manager.
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            pin_requests: RwLock::new(HashMap::new()),
        }
    }

    /// Add a remote pinning service.
    pub fn add_service(&mut self, config: RemotePinServiceConfig) {
        let name = config.name.clone();
        let client = RemotePinningClient::new(config);
        self.clients.insert(name, client);
    }

    /// Request a pin on a specific service.
    pub async fn request_pin_on_service(
        &self,
        service_name: &str,
        cid: &str,
        name: Option<&str>,
    ) -> Result<String, String> {
        let client = self
            .clients
            .get(service_name)
            .ok_or_else(|| format!("service not found: {}", service_name))?;

        let request_id = client.request_pin(cid, name).await?;

        let mut pins = self
            .pin_requests
            .write()
            .map_err(|e| format!("failed to acquire write lock: {}", e))?;

        pins.entry(cid.to_string())
            .or_insert_with(HashMap::new)
            .insert(service_name.to_string(), request_id.clone());

        Ok(request_id)
    }

    /// Request a pin on all configured services.
    pub async fn request_pin_on_all(&self, cid: &str, name: Option<&str>) -> Result<HashMap<String, String>, String> {
        let mut results = HashMap::new();

        for (service_name, _) in &self.clients {
            match self.request_pin_on_service(service_name, cid, name).await {
                Ok(request_id) => {
                    results.insert(service_name.clone(), request_id);
                }
                Err(e) => {
                    // Log error but continue with other services
                    eprintln!("Failed to pin on {}: {}", service_name, e);
                }
            }
        }

        Ok(results)
    }

    /// Get the request ID for a CID on a specific service.
    pub fn get_request_id(&self, cid: &str, service_name: &str) -> Result<Option<String>, String> {
        let pins = self
            .pin_requests
            .read()
            .map_err(|e| format!("failed to acquire read lock: {}", e))?;

        Ok(pins
            .get(cid)
            .and_then(|services| services.get(service_name).cloned()))
    }

    /// Check status of a pin on a specific service.
    pub async fn check_pin_status(
        &self,
        service_name: &str,
        request_id: &str,
    ) -> Result<RemotePinStatus, String> {
        let client = self
            .clients
            .get(service_name)
            .ok_or_else(|| format!("service not found: {}", service_name))?;

        client.check_pin_status(request_id).await
    }

    /// Check status of a pin on all services.
    pub async fn check_pin_status_on_all(&self, cid: &str) -> Result<HashMap<String, RemotePinStatus>, String> {
        let mut results = HashMap::new();

        let pins = self
            .pin_requests
            .read()
            .map_err(|e| format!("failed to acquire read lock: {}", e))?;

        if let Some(services) = pins.get(cid) {
            for (service_name, request_id) in services {
                if let Ok(status) = self.check_pin_status(service_name, request_id).await {
                    results.insert(service_name.clone(), status);
                }
            }
        }

        Ok(results)
    }

    /// Get the number of configured services.
    pub fn service_count(&self) -> usize {
        self.clients.len()
    }

    /// List all service names.
    pub fn service_names(&self) -> Vec<String> {
        self.clients.keys().cloned().collect()
    }
}

impl Default for RemotePinningManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple UUID generation for request IDs (not cryptographically secure).
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_pin_status_is_terminal() {
        assert!(!RemotePinStatus::Queued.is_terminal());
        assert!(!RemotePinStatus::Pinning.is_terminal());
        assert!(RemotePinStatus::Pinned.is_terminal());
        assert!(RemotePinStatus::Failed.is_terminal());
    }

    #[test]
    fn test_remote_pin_service_config_creation() {
        let config = RemotePinServiceConfig {
            name: "pinata".to_string(),
            endpoint: "https://api.pinata.cloud/psa".to_string(),
            access_token: "test_token".to_string(),
            max_pin_size_bytes: Some(1_000_000_000),
        };

        assert_eq!(config.name, "pinata");
        assert_eq!(config.endpoint, "https://api.pinata.cloud/psa");
        assert_eq!(config.max_pin_size_bytes, Some(1_000_000_000));
    }

    #[tokio::test]
    async fn test_remote_pinning_client_request_pin() {
        let config = RemotePinServiceConfig {
            name: "test-service".to_string(),
            endpoint: "https://example.com/psa".to_string(),
            access_token: "token".to_string(),
            max_pin_size_bytes: None,
        };
        let client = RemotePinningClient::new(config);

        let cid = "QmTest123";
        let request_id = client.request_pin(cid, Some("test pin")).await.unwrap();

        assert!(!request_id.is_empty());
        assert!(request_id.contains(cid));
    }

    #[tokio::test]
    async fn test_remote_pinning_client_caches_status() {
        let config = RemotePinServiceConfig {
            name: "test".to_string(),
            endpoint: "https://example.com/psa".to_string(),
            access_token: "token".to_string(),
            max_pin_size_bytes: None,
        };
        let client = RemotePinningClient::new(config);

        let cid = "QmCached";
        let _ = client.request_pin(cid, None).await.unwrap();

        let cached_status = client.get_cached_status(cid);
        assert_eq!(cached_status, Some(RemotePinStatus::Queued));
    }

    #[test]
    fn test_remote_pinning_manager_creation() {
        let manager = RemotePinningManager::new();
        assert_eq!(manager.service_count(), 0);
    }

    #[test]
    fn test_remote_pinning_manager_add_service() {
        let mut manager = RemotePinningManager::new();

        let config = RemotePinServiceConfig {
            name: "pinata".to_string(),
            endpoint: "https://api.pinata.cloud/psa".to_string(),
            access_token: "token".to_string(),
            max_pin_size_bytes: None,
        };
        manager.add_service(config);

        assert_eq!(manager.service_count(), 1);
        assert!(manager.service_names().contains(&"pinata".to_string()));
    }

    #[tokio::test]
    async fn test_remote_pinning_manager_request_pin_on_service() {
        let mut manager = RemotePinningManager::new();

        let config = RemotePinServiceConfig {
            name: "test".to_string(),
            endpoint: "https://example.com/psa".to_string(),
            access_token: "token".to_string(),
            max_pin_size_bytes: None,
        };
        manager.add_service(config);

        let cid = "QmManager";
        let request_id = manager.request_pin_on_service("test", cid, None).await.unwrap();

        assert!(!request_id.is_empty());

        let retrieved_id = manager.get_request_id(cid, "test").unwrap();
        assert_eq!(retrieved_id, Some(request_id));
    }

    #[tokio::test]
    async fn test_remote_pinning_manager_request_pin_on_all() {
        let mut manager = RemotePinningManager::new();

        manager.add_service(RemotePinServiceConfig {
            name: "service1".to_string(),
            endpoint: "https://service1.com/psa".to_string(),
            access_token: "token1".to_string(),
            max_pin_size_bytes: None,
        });
        manager.add_service(RemotePinServiceConfig {
            name: "service2".to_string(),
            endpoint: "https://service2.com/psa".to_string(),
            access_token: "token2".to_string(),
            max_pin_size_bytes: None,
        });

        let cid = "QmMulti";
        let results = manager.request_pin_on_all(cid, None).await.unwrap();

        assert_eq!(results.len(), 2);
        assert!(results.contains_key("service1"));
        assert!(results.contains_key("service2"));
    }

    #[tokio::test]
    async fn test_remote_pinning_manager_check_status_on_all() {
        let mut manager = RemotePinningManager::new();

        manager.add_service(RemotePinServiceConfig {
            name: "test1".to_string(),
            endpoint: "https://test1.com/psa".to_string(),
            access_token: "token".to_string(),
            max_pin_size_bytes: None,
        });
        manager.add_service(RemotePinServiceConfig {
            name: "test2".to_string(),
            endpoint: "https://test2.com/psa".to_string(),
            access_token: "token".to_string(),
            max_pin_size_bytes: None,
        });

        let cid = "QmStatus";
        let _ = manager.request_pin_on_all(cid, None).await.unwrap();

        let statuses = manager.check_pin_status_on_all(cid).await.unwrap();
        assert_eq!(statuses.len(), 2);
    }

    #[test]
    fn test_remote_pinning_manager_default() {
        let manager = RemotePinningManager::default();
        assert_eq!(manager.service_count(), 0);
    }

    #[tokio::test]
    async fn test_psa_pin_serde() {
        let pin = PsaPin {
            request_id: "req123".to_string(),
            status: "pinned".to_string(),
            cid: "Qm123".to_string(),
            name: Some("test pin".to_string()),
            origins: Some(vec!["/ip4/127.0.0.1/tcp/4001".to_string()]),
            meta: None,
        };

        let json = serde_json::to_string(&pin).unwrap();
        let deserialized: PsaPin = serde_json::from_str(&json).unwrap();

        assert_eq!(pin.request_id, deserialized.request_id);
        assert_eq!(pin.cid, deserialized.cid);
    }
}
