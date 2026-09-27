use std::net::TcpListener;
use wrfcoin_arweave::{ArweaveConfig, ArweaveStorageProvider};
use wrfcoin_storage_common::{Metadata, StorageProvider};

#[tokio::test]
async fn store_without_credentials_is_not_configured_and_makes_no_http_call() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let config = ArweaveConfig {
        gateway_url: format!("http://{}", listener.local_addr().unwrap()),
        ..Default::default()
    };
    let provider = ArweaveStorageProvider::new(config);
    let metadata = Metadata {
        content_type: "text/plain".into(),
        size: 4,
        timestamp: 0,
        tags: Default::default(),
    };
    let result = provider.store(b"data", &metadata).await;
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "write reached HTTP listener"
    );
    let err = result.expect_err("unconfigured provider must not claim a write");
    assert!(err.to_string().contains("not configured"), "{err}");
    assert_eq!(provider.stats().await.unwrap().total_stored, 0);
}

#[path = "../../storage-common/tests/support/mock_http.rs"]
mod mock_http;
use wrfcoin_arweave::gateway_client::{ArweaveGatewayClient, TransactionPayload};

#[tokio::test]
async fn unsigned_client_write_is_not_configured_and_makes_no_http_call() {
    let mock = mock_http::MockHttp::new();
    let client = ArweaveGatewayClient::new(&ArweaveConfig {
        gateway_url: mock.url.clone(),
        ..Default::default()
    })
    .unwrap();
    let result = client
        .submit_transaction(&TransactionPayload {
            data: "data".into(),
            tags: vec![],
        })
        .await;
    assert_eq!(mock.requests(), 0, "unsigned write reached HTTP gateway");
    assert!(matches!(
        result,
        Err(wrfcoin_storage_common::StorageError::NotConfigured {
            provider: "arweave",
            ..
        })
    ));
}

#[tokio::test]
async fn arweave_reads_still_work_against_mock_gateway() {
    let mock = mock_http::MockHttp::new();
    let client = ArweaveGatewayClient::new(&ArweaveConfig {
        gateway_url: mock.url.clone(),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(client.get_data("abc").await.unwrap(), b"data");
    assert_eq!(
        client.get_transaction("abc").await.unwrap().id.as_deref(),
        Some("abc")
    );
    assert_eq!(client.get_price(4).await.unwrap(), 42);
    assert_eq!(mock.requests(), 3);
}
