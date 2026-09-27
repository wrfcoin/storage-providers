use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wrfcoin_storage_common::StorageError;

use crate::config::FilecoinConfig;

/// JSON-RPC 2.0 request body.
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Value,
    id: u64,
}

/// JSON-RPC 2.0 response body.
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

/// JSON-RPC error.
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Deal information returned by Lotus.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DealInfo {
    pub deal_id: Option<u64>,
    pub state: Option<u64>,
    pub message: Option<String>,
    pub provider: Option<String>,
    pub piece_cid: Option<CidWrapper>,
    pub size: Option<u64>,
}

/// CID wrapper used in Lotus responses.
#[derive(Debug, Clone, Deserialize)]
pub struct CidWrapper {
    #[serde(rename = "/")]
    pub cid: String,
}

/// HTTP client for the Filecoin Lotus JSON-RPC API.
pub struct LotusClient {
    client: Client,
    base_url: String,
    next_id: AtomicU64,
}

impl LotusClient {
    /// Create a new Lotus client from Filecoin config.
    pub fn new(config: &FilecoinConfig) -> Result<Self, StorageError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| StorageError::ConnectionFailed(format!("reqwest build: {e}")))?;
        Ok(Self {
            client,
            base_url: config.lotus_api_url.clone(),
            next_id: AtomicU64::new(1),
        })
    }

    /// Make a generic JSON-RPC 2.0 call.
    pub async fn rpc_call(&self, method: &str, params: Value) -> Result<Value, StorageError> {
        if method.starts_with("Filecoin.Client") {
            return Err(StorageError::NotConfigured {
                provider: "filecoin",
                reason: "Lotus authentication is not implemented in this build".into(),
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id,
        };

        let resp = self
            .client
            .post(&self.base_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionFailed(format!("rpc {method}: {e}")))?;

        if !resp.status().is_success() {
            return Err(StorageError::ProviderError(format!(
                "rpc {method}: HTTP {}",
                resp.status()
            )));
        }

        let rpc_resp: JsonRpcResponse = resp
            .json()
            .await
            .map_err(|e| StorageError::ProviderError(format!("rpc {method} parse: {e}")))?;

        if let Some(err) = rpc_resp.error {
            return Err(StorageError::ProviderError(format!(
                "rpc {method} error {}: {}",
                err.code, err.message
            )));
        }

        rpc_resp
            .result
            .ok_or_else(|| StorageError::ProviderError(format!("rpc {method}: null result")))
    }

    /// Propose a storage deal via `Filecoin.ClientStartDeal`.
    pub async fn client_start_deal(
        &self,
        data_cid: &str,
        miner: &str,
        price: u64,
        duration: u64,
    ) -> Result<String, StorageError> {
        let params = serde_json::json!([{
            "Data": {"TransferType": "graphsync", "Root": {"/": data_cid}},
            "Miner": miner,
            "EpochPrice": price.to_string(),
            "MinBlocksDuration": duration,
        }]);
        let result = self.rpc_call("Filecoin.ClientStartDeal", params).await?;
        result
            .get("/")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                StorageError::ProviderError("ClientStartDeal: missing CID in response".into())
            })
    }

    /// Get deal info via `Filecoin.ClientGetDealInfo`.
    pub async fn client_get_deal_info(&self, deal_cid: &str) -> Result<DealInfo, StorageError> {
        let params = serde_json::json!([{"/": deal_cid}]);
        let result = self.rpc_call("Filecoin.ClientGetDealInfo", params).await?;
        serde_json::from_value(result)
            .map_err(|e| StorageError::ProviderError(format!("parse DealInfo: {e}")))
    }

    /// List all deals via `Filecoin.ClientListDeals`.
    pub async fn client_list_deals(&self) -> Result<Vec<DealInfo>, StorageError> {
        let result = self
            .rpc_call("Filecoin.ClientListDeals", serde_json::json!([]))
            .await?;
        serde_json::from_value(result)
            .map_err(|e| StorageError::ProviderError(format!("parse deals list: {e}")))
    }
}

/// Build a JSON-RPC request body for testing/inspection.
pub fn build_rpc_request(method: &str, params: Value, id: u64) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lotus_client_builds() {
        let config = FilecoinConfig::default();
        let client = LotusClient::new(&config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_rpc_request_format() {
        let req = build_rpc_request("Filecoin.ClientListDeals", serde_json::json!([]), 1);
        assert_eq!(req["jsonrpc"], "2.0");
        assert_eq!(req["method"], "Filecoin.ClientListDeals");
        assert_eq!(req["id"], 1);
    }

    #[test]
    fn test_deal_info_deserialize() {
        let json = r#"{"DealId": 42, "State": 7, "Message": "active", "Provider": "f01234", "PieceCid": {"/": "bafyabc"}, "Size": 1024}"#;
        let info: DealInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.deal_id, Some(42));
        assert_eq!(info.state, Some(7));
        assert_eq!(info.piece_cid.unwrap().cid, "bafyabc");
    }

    #[test]
    fn test_cid_wrapper_deserialize() {
        let json = r#"{"/": "bafy123"}"#;
        let cid: CidWrapper = serde_json::from_str(json).unwrap();
        assert_eq!(cid.cid, "bafy123");
    }

    #[test]
    fn test_json_rpc_error_deserialize() {
        let json = r#"{"code": -32601, "message": "method not found"}"#;
        let err: JsonRpcError = serde_json::from_str(json).unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "method not found");
    }
}
