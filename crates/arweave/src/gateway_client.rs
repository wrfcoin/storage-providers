use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use wrfcoin_storage_common::StorageError;

use crate::config::ArweaveConfig;

/// Payload for submitting an Arweave transaction.
#[derive(Debug, Serialize)]
pub struct TransactionPayload {
    pub data: String, // base64url-encoded data
    pub tags: Vec<Tag>,
}

/// A name-value tag for Arweave transactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub value: String,
}

/// Transaction info returned by the gateway.
#[derive(Debug, Deserialize)]
pub struct TransactionInfo {
    pub id: Option<String>,
    pub data_size: Option<String>,
    pub reward: Option<String>,
    #[serde(rename = "block_height")]
    pub block_height: Option<u64>,
}

/// HTTP client for the Arweave gateway REST API.
pub struct ArweaveGatewayClient {
    client: Client,
    gateway_url: String,
}

impl ArweaveGatewayClient {
    /// Create a new gateway client from Arweave config.
    pub fn new(config: &ArweaveConfig) -> Result<Self, StorageError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| StorageError::ConnectionFailed(format!("reqwest build: {e}")))?;
        Ok(Self {
            client,
            gateway_url: config.gateway_url.clone(),
        })
    }

    /// Submit a transaction via `POST /tx`.
    pub async fn submit_transaction(
        &self,
        payload: &TransactionPayload,
    ) -> Result<String, StorageError> {
        let url = format!("{}/tx", self.gateway_url);
        let resp = self
            .client
            .post(&url)
            .json(payload)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("submit_tx: {e}")))?;
        if !resp.status().is_success() {
            return Err(StorageError::ProviderError(format!(
                "submit_tx: HTTP {}",
                resp.status()
            )));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| StorageError::ProviderError(format!("submit_tx read: {e}")))?;
        Ok(body)
    }

    /// Get transaction info via `GET /tx/{id}`.
    pub async fn get_transaction(&self, tx_id: &str) -> Result<TransactionInfo, StorageError> {
        let url = format!("{}/tx/{}", self.gateway_url, tx_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("get_tx: {e}")))?;
        if !resp.status().is_success() {
            return Err(StorageError::NotFound(format!(
                "get_tx: HTTP {} for {tx_id}",
                resp.status()
            )));
        }
        resp.json::<TransactionInfo>()
            .await
            .map_err(|e| StorageError::ProviderError(format!("get_tx parse: {e}")))
    }

    /// Get raw data via `GET /{id}`.
    pub async fn get_data(&self, tx_id: &str) -> Result<Vec<u8>, StorageError> {
        let url = format!("{}/{}", self.gateway_url, tx_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("get_data: {e}")))?;
        if !resp.status().is_success() {
            return Err(StorageError::NotFound(format!(
                "get_data: HTTP {} for {tx_id}",
                resp.status()
            )));
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| StorageError::ProviderError(format!("get_data read: {e}")))
    }

    /// Get the price in winston for storing the given number of bytes via `GET /price/{bytes}`.
    pub async fn get_price(&self, bytes: u64) -> Result<u64, StorageError> {
        let url = format!("{}/price/{}", self.gateway_url, bytes);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("get_price: {e}")))?;
        if !resp.status().is_success() {
            return Err(StorageError::ProviderError(format!(
                "get_price: HTTP {}",
                resp.status()
            )));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| StorageError::ProviderError(format!("get_price read: {e}")))?;
        body.trim()
            .parse::<u64>()
            .map_err(|e| StorageError::ProviderError(format!("get_price parse: {e}")))
    }
}

/// Build tags from a HashMap for convenience.
pub fn tags_from_map(map: &HashMap<String, String>) -> Vec<Tag> {
    map.iter()
        .map(|(k, v)| Tag {
            name: k.clone(),
            value: v.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_client_builds() {
        let config = ArweaveConfig::default();
        let client = ArweaveGatewayClient::new(&config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_transaction_payload_serializes() {
        let payload = TransactionPayload {
            data: "aGVsbG8".to_string(),
            tags: vec![Tag {
                name: "Content-Type".to_string(),
                value: "text/plain".to_string(),
            }],
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("aGVsbG8"));
        assert!(json.contains("Content-Type"));
    }

    #[test]
    fn test_transaction_info_deserialize() {
        let json = r#"{"id":"abc123","data_size":"1024","reward":"5000","block_height":1000}"#;
        let info: TransactionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id.unwrap(), "abc123");
        assert_eq!(info.block_height, Some(1000));
    }

    #[test]
    fn test_tags_from_map() {
        let mut map = HashMap::new();
        map.insert("key".to_string(), "value".to_string());
        let tags = tags_from_map(&map);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "key");
    }
}
