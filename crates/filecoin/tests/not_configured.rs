use std::net::TcpListener;
use wrfcoin_filecoin::{FilecoinConfig, FilecoinStorageProvider};
use wrfcoin_storage_common::{Metadata, StorageProvider};

#[tokio::test]
async fn store_without_credentials_is_not_configured_and_makes_no_http_call() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let config = FilecoinConfig {
        lotus_api_url: format!("http://{}", listener.local_addr().unwrap()),
        ..Default::default()
    };
    let provider = FilecoinStorageProvider::new(config);
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
use wrfcoin_filecoin::rpc_client::LotusClient;
use wrfcoin_storage_common::{ContentId, StorageError};

#[tokio::test]
async fn token_does_not_enable_unimplemented_auth_or_writes() {
    let mock = mock_http::MockHttp::new();
    for token in [None, Some(String::new()), Some("dummy-secret".into())] {
        let config = FilecoinConfig {
            lotus_api_url: mock.url.clone(),
            api_token: token,
            ..Default::default()
        };
        assert!(!format!("{config:?}").contains("dummy-secret"));
        assert!(!serde_json::to_string(&config)
            .unwrap()
            .contains("dummy-secret"));
        let provider = FilecoinStorageProvider::new(config.clone());
        let result = provider
            .delete(&ContentId {
                provider: "filecoin".into(),
                hash: "abc".into(),
            })
            .await;
        assert!(matches!(result, Err(StorageError::NotConfigured { .. })));
        let metadata = Metadata {
            content_type: "text/plain".into(),
            size: 4,
            timestamp: 0,
            tags: Default::default(),
        };
        assert!(matches!(
            provider.store(b"data", &metadata).await,
            Err(StorageError::NotConfigured { .. })
        ));
        let client = LotusClient::new(&config).unwrap();
        let result = client.client_start_deal("abc", "miner", 1, 1).await;
        assert_eq!(mock.requests(), 0, "unauthenticated write reached Lotus");
        assert!(matches!(result, Err(StorageError::NotConfigured { .. })));
        let result = client
            .rpc_call("Filecoin.ClientStartDeal", serde_json::json!([]))
            .await;
        assert_eq!(mock.requests(), 0);
        assert!(matches!(result, Err(StorageError::NotConfigured { .. })));
    }
}

#[tokio::test]
async fn unconfigured_health_is_unhealthy_without_network_or_invented_latency() {
    let mock = mock_http::MockHttp::new();
    let provider = FilecoinStorageProvider::new(FilecoinConfig {
        lotus_api_url: mock.url.clone(),
        ..Default::default()
    });
    let health = provider.health_check().await.unwrap();
    assert_eq!(mock.requests(), 0);
    assert!(
        !health.healthy,
        "unconfigured provider fabricated healthy status"
    );
    assert_eq!(health.latency_ms, None);
    assert!(health.message.unwrap().starts_with("not configured:"));
}

#[test]
fn debug_keeps_nonsecret_configuration_and_redacts_token() {
    let config = FilecoinConfig {
        api_token: Some("dummy-secret".into()),
        ..Default::default()
    };
    let debug = format!("{config:?}");
    assert!(!debug.contains("dummy-secret"));
    for field in [
        "lotus_api_url",
        "wallet_address",
        "max_price_per_gib",
        "deal_duration_epochs",
        "verified_deals",
    ] {
        assert!(debug.contains(field), "missing non-secret field {field}");
    }
}
