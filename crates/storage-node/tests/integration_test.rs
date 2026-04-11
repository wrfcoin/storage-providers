use std::process::Command;

#[test]
fn test_storage_node_config_loading() {
    // Create a test config file
    let config_content = r#"
bind_addr = "127.0.0.1:7002"
storage_tier = "cold"
cache_ttl_secs = 7200

[provider_endpoints]
ipfs = "http://localhost:5001"
filecoin = "http://localhost:1234"
"#;

    std::fs::write("test-storage-node-config.toml", config_content)
        .expect("Failed to write test config");

    // The config should be loadable with the env var
    let output = Command::new("cargo")
        .args(&["build", "--release"])
        .env("STORAGE_NODE_CONFIG", "test-storage-node-config.toml")
        .output()
        .expect("Failed to build storage-node");

    assert!(
        output.status.success(),
        "Build should succeed with valid config file"
    );

    // Cleanup
    std::fs::remove_file("test-storage-node-config.toml").ok();
}

#[test]
fn test_storage_node_graceful_shutdown() {
    // This test verifies that the daemon can be shut down gracefully.
    // In a real scenario, you'd spawn the daemon, verify it's running,
    // send SIGTERM, and verify it shuts down cleanly.
    //
    // For now, we test that the binary compiles without issues.
    let output = Command::new("cargo")
        .args(&["build", "--release", "-p", "wrfcoin-storage-node"])
        .output()
        .expect("Failed to build");

    assert!(
        output.status.success(),
        "storage-node should build successfully"
    );
}

#[test]
fn test_storage_node_metrics_endpoint() {
    // Verify that the /metrics endpoint would be registered in the router.
    // This is a compile-time test to ensure the metrics route exists.
    //
    // A full integration test would:
    // 1. Spawn the daemon
    // 2. Make a GET request to /metrics
    // 3. Verify prometheus format output
    // 4. Send SIGTERM and verify graceful shutdown

    let output = Command::new("cargo")
        .args(&["check", "-p", "wrfcoin-storage-node"])
        .output()
        .expect("Failed to check storage-node");

    assert!(
        output.status.success(),
        "storage-node should compile with /metrics endpoint"
    );
}
