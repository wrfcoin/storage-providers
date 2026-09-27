//! Integration tests for the failover router and health aggregator.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wrfcoin_storage_common::error::StorageError;
use wrfcoin_storage_common::failover::FailoverRouter;
use wrfcoin_storage_common::health::HealthAggregator;
use wrfcoin_storage_common::integrity::IntegrityVerifier;
use wrfcoin_storage_common::traits::StorageProvider;
use wrfcoin_storage_common::types::{ContentId, HealthStatus, Metadata, StorageStats};

// ---------------------------------------------------------------------------
// Mock provider
// ---------------------------------------------------------------------------

/// Configurable mock that can succeed, fail, or return corrupted data.
struct MockProvider {
    name: String,
    /// When true, all mutable operations return ConnectionFailed.
    should_fail: Mutex<bool>,
    /// When set, `retrieve` returns this instead of the real stored data.
    corrupt_data: Mutex<Option<Vec<u8>>>,
    /// In-memory store.
    store: Mutex<HashMap<String, Vec<u8>>>,
    /// Tracks how many times `store` was called.
    store_calls: AtomicU64,
    /// Tracks how many times `retrieve` was called.
    retrieve_calls: AtomicU64,
    /// Healthy flag for health_check.
    healthy: Mutex<bool>,
}

impl MockProvider {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            should_fail: Mutex::new(false),
            corrupt_data: Mutex::new(None),
            store: Mutex::new(HashMap::new()),
            store_calls: AtomicU64::new(0),
            retrieve_calls: AtomicU64::new(0),
            healthy: Mutex::new(true),
        }
    }

    fn set_fail(&self, fail: bool) {
        *self.should_fail.lock().unwrap() = fail;
    }

    fn set_corrupt_data(&self, data: Option<Vec<u8>>) {
        *self.corrupt_data.lock().unwrap() = data;
    }

    fn set_healthy(&self, h: bool) {
        *self.healthy.lock().unwrap() = h;
    }

    fn store_call_count(&self) -> u64 {
        self.store_calls.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    fn retrieve_call_count(&self) -> u64 {
        self.retrieve_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl StorageProvider for MockProvider {
    async fn store(&self, data: &[u8], _metadata: &Metadata) -> Result<ContentId, StorageError> {
        self.store_calls.fetch_add(1, Ordering::SeqCst);
        if *self.should_fail.lock().unwrap() {
            return Err(StorageError::ConnectionFailed(format!(
                "{} is down",
                self.name
            )));
        }
        let hash = IntegrityVerifier::hash(data);
        self.store
            .lock()
            .unwrap()
            .insert(hash.clone(), data.to_vec());
        Ok(ContentId {
            provider: self.name.clone(),
            hash,
        })
    }

    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        self.retrieve_calls.fetch_add(1, Ordering::SeqCst);
        if *self.should_fail.lock().unwrap() {
            return Err(StorageError::ConnectionFailed(format!(
                "{} is down",
                self.name
            )));
        }
        // Return corrupt data if configured.
        if let Some(ref bad) = *self.corrupt_data.lock().unwrap() {
            return Ok(bad.clone());
        }
        self.store
            .lock()
            .unwrap()
            .get(&id.hash)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(id.hash.clone()))
    }

    async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
        if *self.should_fail.lock().unwrap() {
            return Err(StorageError::ConnectionFailed(format!(
                "{} is down",
                self.name
            )));
        }
        Ok(self.store.lock().unwrap().contains_key(&id.hash))
    }

    async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
        if *self.should_fail.lock().unwrap() {
            return Err(StorageError::ConnectionFailed(format!(
                "{} is down",
                self.name
            )));
        }
        self.store.lock().unwrap().remove(&id.hash);
        Ok(())
    }

    async fn health_check(&self) -> Result<HealthStatus, StorageError> {
        let h = *self.healthy.lock().unwrap();
        Ok(HealthStatus {
            healthy: h,
            provider: self.name.clone(),
            latency_ms: Some(1),
            message: None,
        })
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        Ok(StorageStats::default())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_metadata() -> Metadata {
    Metadata {
        content_type: "application/octet-stream".to_string(),
        size: 0,
        timestamp: 1_000_000,
        tags: HashMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_store_uses_primary_provider() {
    let primary = Arc::new(MockProvider::new("primary"));
    let secondary = Arc::new(MockProvider::new("secondary"));

    let mut router = FailoverRouter::new(3, Duration::from_secs(30));
    router.add_provider("primary", primary.clone(), 0);
    router.add_provider("secondary", secondary.clone(), 1);

    let data = b"weather observation 42";
    let cid = router.store(data, &test_metadata()).await.unwrap();

    assert_eq!(cid.provider, "primary");
    assert_eq!(primary.store_call_count(), 1);
    assert_eq!(secondary.store_call_count(), 0);
}

#[tokio::test]
async fn test_store_failover_on_primary_failure() {
    let primary = Arc::new(MockProvider::new("primary"));
    let secondary = Arc::new(MockProvider::new("secondary"));
    primary.set_fail(true);

    let mut router = FailoverRouter::new(3, Duration::from_secs(30));
    router.add_provider("primary", primary.clone(), 0);
    router.add_provider("secondary", secondary.clone(), 1);

    let data = b"failover data";
    let cid = router.store(data, &test_metadata()).await.unwrap();

    assert_eq!(cid.provider, "secondary");
    assert_eq!(primary.store_call_count(), 1);
    assert_eq!(secondary.store_call_count(), 1);

    let m = router.metrics();
    assert_eq!(m.failover_count, 1);
    assert_eq!(m.primary_successes, 0);
}

#[tokio::test]
async fn test_circuit_breaker_trips_after_threshold() {
    let primary = Arc::new(MockProvider::new("primary"));
    let secondary = Arc::new(MockProvider::new("secondary"));
    primary.set_fail(true);

    let mut router = FailoverRouter::new(3, Duration::from_secs(60));
    router.add_provider("primary", primary.clone(), 0);
    router.add_provider("secondary", secondary.clone(), 1);

    let meta = test_metadata();

    // Trigger 3 failures on primary (threshold = 3).
    for _ in 0..3 {
        let _ = router.store(b"x", &meta).await.unwrap();
    }

    // Primary should now be circuit-broken; 4th request should NOT call primary.
    let prev_primary_calls = primary.store_call_count();
    let _ = router.store(b"y", &meta).await.unwrap();
    assert_eq!(primary.store_call_count(), prev_primary_calls);
    // Secondary should have handled all 4 successful stores.
    assert_eq!(secondary.store_call_count(), 4);
}

#[tokio::test]
async fn test_circuit_breaker_resets_after_cooldown() {
    let primary = Arc::new(MockProvider::new("primary"));
    let secondary = Arc::new(MockProvider::new("secondary"));
    primary.set_fail(true);

    // Use 0ms cooldown so the breaker immediately transitions to HalfOpen.
    let mut router = FailoverRouter::new(2, Duration::from_millis(0));
    router.add_provider("primary", primary.clone(), 0);
    router.add_provider("secondary", secondary.clone(), 1);

    let meta = test_metadata();

    // Trip the breaker (2 failures).
    let _ = router.store(b"a", &meta).await.unwrap();
    let _ = router.store(b"b", &meta).await.unwrap();

    // Now heal the primary.
    primary.set_fail(false);

    // Next request: cooldown=0 => HalfOpen => tries primary => succeeds => Closed.
    let cid = router.store(b"c", &meta).await.unwrap();
    assert_eq!(cid.provider, "primary");
}

#[tokio::test]
async fn test_retrieve_with_integrity_check() {
    let provider = Arc::new(MockProvider::new("ipfs"));

    let mut router = FailoverRouter::new(3, Duration::from_secs(30));
    router.add_provider("ipfs", provider.clone(), 0);

    let data = b"verified payload";
    let hash = IntegrityVerifier::hash(data);

    let cid = router.store(data, &test_metadata()).await.unwrap();
    let retrieved = router.retrieve(&cid, Some(&hash)).await.unwrap();
    assert_eq!(retrieved, data);

    let m = router.metrics();
    assert_eq!(m.integrity_checks, 1);
    assert_eq!(m.integrity_failures, 0);
}

#[tokio::test]
async fn test_retrieve_integrity_mismatch_rejected() {
    let corrupt_provider = Arc::new(MockProvider::new("corrupt"));
    let good_provider = Arc::new(MockProvider::new("good"));

    let data = b"original data";
    let hash = IntegrityVerifier::hash(data);

    // Store on both.
    let meta = test_metadata();
    let mut router = FailoverRouter::new(5, Duration::from_secs(30));
    router.add_provider("corrupt", corrupt_provider.clone(), 0);
    router.add_provider("good", good_provider.clone(), 1);

    let cid = router.store(data, &meta).await.unwrap();

    // Also store in the good provider directly so retrieve can find it.
    good_provider.store(data, &meta).await.unwrap();

    // Make the corrupt provider return bad data.
    corrupt_provider.set_corrupt_data(Some(b"tampered".to_vec()));

    let retrieved = router.retrieve(&cid, Some(&hash)).await.unwrap();
    assert_eq!(retrieved, data);

    let m = router.metrics();
    assert_eq!(m.integrity_checks, 2); // first check failed, second succeeded
    assert_eq!(m.integrity_failures, 1);
    assert_eq!(m.failover_count, 1);
}

#[tokio::test]
async fn test_all_providers_down_returns_error() {
    let p1 = Arc::new(MockProvider::new("p1"));
    let p2 = Arc::new(MockProvider::new("p2"));
    p1.set_fail(true);
    p2.set_fail(true);

    let mut router = FailoverRouter::new(3, Duration::from_secs(30));
    router.add_provider("p1", p1, 0);
    router.add_provider("p2", p2, 1);

    let result = router.store(b"data", &test_metadata()).await;
    assert!(result.is_err());

    let m = router.metrics();
    assert_eq!(m.total_failures, 1);
}

#[tokio::test]
async fn test_health_aggregation() {
    let healthy = Arc::new(MockProvider::new("healthy"));
    let sick = Arc::new(MockProvider::new("sick"));
    sick.set_healthy(false);

    let mut agg = HealthAggregator::new();
    agg.add_provider("healthy", healthy);
    agg.add_provider("sick", sick);

    let result = agg.check_all().await;

    assert!(result.overall_healthy);
    assert_eq!(result.healthy_count, 1);
    assert_eq!(result.total_count, 2);

    // Verify individual entries.
    let healthy_entry = result
        .providers
        .iter()
        .find(|e| e.name == "healthy")
        .unwrap();
    assert!(healthy_entry.status.as_ref().unwrap().healthy);

    let sick_entry = result.providers.iter().find(|e| e.name == "sick").unwrap();
    assert!(!sick_entry.status.as_ref().unwrap().healthy);
}

#[tokio::test]
async fn test_health_aggregator_check_single_provider() {
    let p = Arc::new(MockProvider::new("ipfs"));

    let mut agg = HealthAggregator::new();
    agg.add_provider("ipfs", p);

    let result = agg.check_provider("ipfs").await;
    assert!(result.is_some());
    assert!(result.unwrap().unwrap().healthy);

    let missing = agg.check_provider("nonexistent").await;
    assert!(missing.is_none());
}

#[tokio::test]
async fn test_failover_metrics_tracked() {
    let primary = Arc::new(MockProvider::new("primary"));
    let secondary = Arc::new(MockProvider::new("secondary"));

    let mut router = FailoverRouter::new(5, Duration::from_secs(30));
    router.add_provider("primary", primary.clone(), 0);
    router.add_provider("secondary", secondary.clone(), 1);

    let meta = test_metadata();

    // 2 successful stores on primary.
    router.store(b"a", &meta).await.unwrap();
    router.store(b"b", &meta).await.unwrap();

    // Make primary fail, causing failover.
    primary.set_fail(true);
    router.store(b"c", &meta).await.unwrap();

    // All providers down.
    secondary.set_fail(true);
    let _ = router.store(b"d", &meta).await;

    let m = router.metrics();
    assert_eq!(m.total_requests, 4);
    assert_eq!(m.primary_successes, 2);
    assert_eq!(m.failover_count, 1);
    assert_eq!(m.total_failures, 1);
}

#[tokio::test]
async fn test_priority_ordering_respected() {
    let low = Arc::new(MockProvider::new("low-priority"));
    let high = Arc::new(MockProvider::new("high-priority"));
    let mid = Arc::new(MockProvider::new("mid-priority"));

    let mut router = FailoverRouter::new(3, Duration::from_secs(30));
    // Insert out of order to verify sorting.
    router.add_provider("low-priority", low.clone(), 10);
    router.add_provider("high-priority", high.clone(), 1);
    router.add_provider("mid-priority", mid.clone(), 5);

    let cid = router.store(b"ordered", &test_metadata()).await.unwrap();

    assert_eq!(cid.provider, "high-priority");
    assert_eq!(high.store_call_count(), 1);
    assert_eq!(mid.store_call_count(), 0);
    assert_eq!(low.store_call_count(), 0);
}

#[tokio::test]
async fn test_failover_router_health_via_trait() {
    let p = Arc::new(MockProvider::new("only"));

    let mut router = FailoverRouter::new(3, Duration::from_secs(30));
    router.add_provider("only", p, 0);

    // Use the StorageProvider trait method.
    let status = StorageProvider::health_check(&router).await.unwrap();
    assert!(status.healthy);
    assert_eq!(status.provider, "failover-router");
    assert!(status.message.unwrap().contains("1/1"));
}

#[tokio::test]
async fn test_exists_checks_all_providers() {
    let p1 = Arc::new(MockProvider::new("p1"));
    let p2 = Arc::new(MockProvider::new("p2"));

    let mut router = FailoverRouter::new(3, Duration::from_secs(30));
    router.add_provider("p1", p1.clone(), 0);
    router.add_provider("p2", p2.clone(), 1);

    // Store only on p2 directly.
    let meta = test_metadata();
    let cid = p2.store(b"data", &meta).await.unwrap();

    // Router should find it via p2 even though p1 doesn't have it.
    let found = router.exists(&cid).await.unwrap();
    assert!(found);

    // Non-existent content.
    let missing_cid = ContentId {
        provider: "p1".to_string(),
        hash: "nonexistent".to_string(),
    };
    let not_found = router.exists(&missing_cid).await.unwrap();
    assert!(!not_found);
}
