use axum::{body::Bytes, extract::State, http::HeaderMap, routing::post, Json, Router};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

struct Daemon {
    child: Child,
    config: PathBuf,
    url: String,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.config);
    }
}
impl Daemon {
    fn spawn(kubo: &str, envs: &[(&str, &str)]) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let config = std::env::temp_dir().join(format!(
            "storage-profile-{}-{}.toml",
            std::process::id(),
            addr.port()
        ));
        std::fs::write(
            &config,
            format!("bind_addr = \"{addr}\"\n[provider_endpoints]\nipfs = \"{kubo}\"\n"),
        )
        .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_storage-node"))
            .env_clear()
            .env("STORAGE_NODE_CONFIG", &config)
            .envs(envs.iter().copied())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self {
            child,
            config,
            url: format!("http://{addr}"),
        }
    }
    async fn ready(&mut self) {
        for _ in 0..200 {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "daemon exited during startup"
            );
            if reqwest::get(format!("{}/stats", self.url)).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("daemon did not start");
    }
    async fn json(&self, path: &str) -> (u16, Value) {
        let r = reqwest::get(format!("{}{path}", self.url)).await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap())
    }
}
#[derive(Default)]
struct Calls {
    add: AtomicUsize,
    cat: AtomicUsize,
    auth: AtomicUsize,
}
struct Kubo {
    url: String,
    calls: Arc<Calls>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Kubo {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn kubo() -> Kubo {
    let calls = Arc::new(Calls::default());
    let app = Router::new()
        .route(
            "/api/v0/id",
            post(|| async { Json(json!({"ID":"mock-kubo"})) }),
        )
        .route(
            "/api/v0/add",
            post(
                |State(c): State<Arc<Calls>>, h: HeaderMap, body: Bytes| async move {
                    c.add.fetch_add(1, Ordering::SeqCst);
                    if h.contains_key("authorization") {
                        c.auth.fetch_add(1, Ordering::SeqCst);
                    }
                    assert!(body.windows(4).any(|b| b == b"data"));
                    Json(json!({"Name":"data","Hash":"bafy-kubo-opaque-cid","Size":"4"}))
                },
            ),
        )
        .route(
            "/api/v0/cat",
            post(|State(c): State<Arc<Calls>>| async move {
                c.cat.fetch_add(1, Ordering::SeqCst);
                "data"
            }),
        )
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Kubo { url, calls, task }
}
fn provider<'a>(body: &'a Value, name: &str) -> &'a Value {
    body["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap()
}
async fn store(d: &Daemon) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/api/store", d.url))
        .json(&json!({"data":"ZGF0YQ==","content_type":"text/plain"}))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn boots_ipfs_only_by_default() {
    let k = kubo().await;
    let mut d = Daemon::spawn(&k.url, &[]);
    d.ready().await;
    let (status, body) = d.json("/health").await;
    assert_eq!(status, 200);
    assert_eq!(provider(&body, "ipfs")["posture"], "active");
    assert_eq!(provider(&body, "ipfs")["healthy"], true);
    for name in ["filecoin", "arweave"] {
        assert_eq!(provider(&body, name)["posture"], "not_in_testnet");
        assert!(provider(&body, name)["healthy"].is_null());
    }
}
#[tokio::test]
async fn store_then_retrieve_roundtrip_via_mock_kubo() {
    let k = kubo().await;
    let mut d = Daemon::spawn(&k.url, &[]);
    d.ready().await;
    let r = store(&d).await;
    assert_eq!(r.status(), 201);
    let b: Value = r.json().await.unwrap();
    assert_eq!(k.calls.add.load(Ordering::SeqCst), 1);
    let hash = b["content_id"]["hash"].as_str().unwrap();
    drop(d);
    let mut d = Daemon::spawn(&k.url, &[]);
    d.ready().await;
    let r = reqwest::get(format!("{}/api/retrieve/ipfs:{hash}", d.url))
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.bytes().await.unwrap(), "data");
    assert_eq!(k.calls.cat.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn filecoin_request_is_501_not_in_testnet() {
    let k = kubo().await;
    let mut d = Daemon::spawn(&k.url, &[]);
    d.ready().await;
    for name in ["filecoin", "arweave"] {
        let r = reqwest::Client::new()
            .post(format!("{}/api/store?provider={name}", d.url))
            .json(&json!({"data":"ZGF0YQ==","content_type":"text/plain"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 501);
        let b: Value = r.json().await.unwrap();
        assert_eq!(b["unavailable"], true);
        assert_eq!(b["provider"], name);
        assert_eq!(b["reason"], "not in this testnet");
        let (s, b) = d
            .json(&format!("/api/retrieve/ipfs:abc?provider={name}"))
            .await;
        assert_eq!(s, 501);
        assert_eq!(b["reason"], "not in this testnet");
    }
    assert_eq!(k.calls.add.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn stats_reports_not_in_testnet_with_null_health() {
    let k = kubo().await;
    let mut d = Daemon::spawn(&k.url, &[]);
    d.ready().await;
    let (_, b) = d.json("/stats").await;
    for name in ["filecoin", "arweave"] {
        assert_eq!(provider(&b, name)["posture"], "not_in_testnet");
        assert!(provider(&b, name)["healthy"].is_null());
    }
    assert!(provider(&b, "ipfs")["healthy"].is_null());
    let (_, b) = d.json("/api/cost/estimate?size_bytes=4").await;
    assert_eq!(b["estimate_only"], true);
    assert_eq!(b["estimates"].as_array().unwrap().len(), 1);
    drop(d);
    // Full profile can list an incomplete provider, but missing probe evidence must be false.
    let mut d = Daemon::spawn(
        &k.url,
        &[
            ("STORAGE_NODE_PROFILE", "full"),
            ("STORAGE_PROVIDERS", "ipfs,filecoin"),
        ],
    );
    d.ready().await;
    let (_, body) = d.json("/api/analytics/snapshot").await;
    assert_eq!(provider(&body, "filecoin")["healthy"], false);
}
#[tokio::test]
async fn health_is_503_when_kubo_down() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut d = Daemon::spawn(&url, &[]);
    d.ready().await;
    let (s, b) = tokio::time::timeout(Duration::from_secs(3), d.json("/health"))
        .await
        .unwrap();
    assert_eq!(s, 503);
    assert_eq!(provider(&b, "ipfs")["healthy"], false);
    assert!(
        !store(&d).await.status().is_success(),
        "unreachable Kubo must not produce a fake successful write"
    );
    drop(d);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let app = Router::new().route(
            "/api/v0/id",
            post(|| async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                Json(json!({"ID":"slow"}))
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });
    let mut d = Daemon::spawn(&url, &[]);
    d.ready().await;
    let (status, _) = tokio::time::timeout(Duration::from_secs(3), d.json("/health"))
        .await
        .expect("Kubo health probe exceeded two-second timeout");
    task.abort();
    assert_eq!(status, 503);
}
#[tokio::test]
async fn testnet_profile_refuses_filecoin_in_provider_list() {
    for name in ["filecoin", "arweave"] {
        let mut d = Daemon::spawn(
            "http://127.0.0.1:1",
            &[("STORAGE_PROVIDERS", name), ("DEFAULT_PROVIDER", name)],
        );
        let mut exited = false;
        for _ in 0..200 {
            if let Some(status) = d.child.try_wait().unwrap() {
                assert!(!status.success());
                exited = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(exited, "paid provider was allowed to boot");
    }
}
#[tokio::test]
async fn remote_pinning_forced_off_in_testnet_profile() {
    let k = kubo().await;
    let mut d = Daemon::spawn(
        &k.url,
        &[
            ("IPFS_REMOTE_PINNING", "true"),
            ("IPFS_IS_LOCAL", "false"),
            ("IPFS_API_TOKEN", "dummy-must-not-be-read"),
        ],
    );
    d.ready().await;
    let (_, b) = d.json("/stats").await;
    assert_eq!(b["remote_pinning_enabled"], false);
    assert_eq!(store(&d).await.status(), 201);
    assert_eq!(k.calls.add.load(Ordering::SeqCst), 1);
    assert_eq!(k.calls.auth.load(Ordering::SeqCst), 0);
}
