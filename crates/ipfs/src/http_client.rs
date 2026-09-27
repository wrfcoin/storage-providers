use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use wrfcoin_storage_common::StorageError;

use crate::config::IpfsConfig;

/// Response from the Kubo `/api/v0/add` endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AddResponse {
    pub name: String,
    pub hash: String,
    pub size: String,
}

/// Response from the Kubo `/api/v0/id` endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IdResponse {
    #[serde(rename = "ID", alias = "Id")]
    pub id: String,
    pub agent_version: Option<String>,
}

/// HTTP client for the IPFS Kubo API.
pub struct KuboClient {
    client: Client,
    base_url: String,
    /// Optional bearer token for API authentication.
    api_token: Option<String>,
}

impl KuboClient {
    /// Create a new Kubo client from IPFS config.
    ///
    /// Returns an error if `api_token` is `Some("")` (fail-closed on empty tokens).
    pub fn new(config: &IpfsConfig) -> Result<Self, StorageError> {
        if let Some(ref token) = config.api_token {
            if token.is_empty() {
                return Err(StorageError::ConfigError(
                    "IPFS API token is configured but empty — refusing to proceed".to_string(),
                ));
            }
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .pool_max_idle_per_host(config.max_connections as usize)
            .build()
            .map_err(|e| StorageError::ConnectionFailed(format!("reqwest build: {e}")))?;
        Ok(Self {
            client,
            base_url: config.api_url.clone(),
            api_token: config.api_token.clone(),
        })
    }

    /// Return a request builder with auth header attached (if token is configured).
    fn post(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.client.post(url);
        if let Some(ref token) = self.api_token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req
    }

    /// Upload data to IPFS via `POST /api/v0/add`.
    pub async fn add(&self, data: Vec<u8>) -> Result<AddResponse, StorageError> {
        let url = format!("{}/api/v0/add", self.base_url);
        let part = reqwest::multipart::Part::bytes(data).file_name("data");
        let form = reqwest::multipart::Form::new().part("file", part);
        let resp = self
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("add: {e}")))?;
        if !resp.status().is_success() {
            return Err(StorageError::ProviderError(format!(
                "add: HTTP {}",
                resp.status()
            )));
        }
        resp.json::<AddResponse>()
            .await
            .map_err(|e| StorageError::ProviderError(format!("add parse: {e}")))
    }

    /// Retrieve data from IPFS via `POST /api/v0/cat?arg={cid}`.
    pub async fn cat(&self, cid: &str) -> Result<Vec<u8>, StorageError> {
        let url = format!("{}/api/v0/cat?arg={}", self.base_url, cid);
        let resp = self
            .post(&url)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("cat: {e}")))?;
        if !resp.status().is_success() {
            return Err(StorageError::NotFound(format!(
                "cat: HTTP {} for {cid}",
                resp.status()
            )));
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| StorageError::ProviderError(format!("cat read: {e}")))
    }

    /// Pin content via `POST /api/v0/pin/add?arg={cid}`.
    pub async fn pin_add(&self, cid: &str) -> Result<(), StorageError> {
        let url = format!("{}/api/v0/pin/add?arg={}", self.base_url, cid);
        let resp = self
            .post(&url)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("pin/add: {e}")))?;
        if !resp.status().is_success() {
            return Err(StorageError::ProviderError(format!(
                "pin/add: HTTP {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Unpin content via `POST /api/v0/pin/rm?arg={cid}`.
    pub async fn pin_rm(&self, cid: &str) -> Result<(), StorageError> {
        let url = format!("{}/api/v0/pin/rm?arg={}", self.base_url, cid);
        let resp = self
            .post(&url)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("pin/rm: {e}")))?;
        if !resp.status().is_success() {
            return Err(StorageError::ProviderError(format!(
                "pin/rm: HTTP {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Health check via `POST /api/v0/id`.
    pub async fn id(&self) -> Result<IdResponse, StorageError> {
        let url = format!("{}/api/v0/id", self.base_url);
        let resp = self
            .post(&url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("id: {e}")))?;
        if !resp.status().is_success() {
            return Err(StorageError::ProviderError(format!(
                "id: HTTP {}",
                resp.status()
            )));
        }
        resp.json::<IdResponse>()
            .await
            .map_err(|e| StorageError::ProviderError(format!("id parse: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kubo_client_builds() {
        let config = IpfsConfig::default();
        let client = KuboClient::new(&config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_kubo_client_rejects_empty_token() {
        let config = IpfsConfig {
            api_token: Some("".to_string()),
            ..Default::default()
        };
        let result = KuboClient::new(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_kubo_client_accepts_valid_token() {
        let config = IpfsConfig {
            api_token: Some("secret-token".to_string()),
            ..Default::default()
        };
        let client = KuboClient::new(&config);
        assert!(client.is_ok());
        assert_eq!(client.unwrap().api_token.as_deref(), Some("secret-token"));
    }

    #[test]
    fn test_kubo_client_accepts_no_token() {
        let config = IpfsConfig {
            api_token: None,
            ..Default::default()
        };
        let client = KuboClient::new(&config);
        assert!(client.is_ok());
        assert!(client.unwrap().api_token.is_none());
    }

    #[test]
    fn test_add_response_deserialize() {
        let json = r#"{"Name":"data","Hash":"QmXyz123","Size":"42"}"#;
        let resp: AddResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.hash, "QmXyz123");
        assert_eq!(resp.size, "42");
    }

    #[test]
    fn test_id_response_deserialize() {
        let json = r#"{"Id":"12D3KooW...","AgentVersion":"kubo/0.24.0"}"#;
        let resp: IdResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "12D3KooW...");
        assert_eq!(resp.agent_version.unwrap(), "kubo/0.24.0");
    }
}
