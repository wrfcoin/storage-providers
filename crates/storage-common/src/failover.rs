//! Failover router with circuit breakers and integrity verification.
//!
//! [`FailoverRouter`] wraps multiple [`StorageProvider`] implementations and
//! tries them in priority order. Each provider is protected by a
//! [`CircuitBreaker`] that temporarily removes unhealthy backends from the
//! rotation.

use crate::error::StorageError;
use crate::integrity::IntegrityVerifier;
use crate::traits::StorageProvider;
use crate::types::{ContentId, HealthStatus, Metadata, StorageStats};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Circuit breaker
// ---------------------------------------------------------------------------

/// Per-provider circuit breaker preventing repeated calls to failing backends.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    failure_count: u32,
    failure_threshold: u32,
    last_failure: Option<Instant>,
    cooldown: Duration,
    state: CircuitState,
}

/// Tri-state model for the circuit breaker.
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    /// Normal operation — requests flow through.
    Closed,
    /// Provider is considered down — requests are skipped.
    Open,
    /// Cooldown elapsed — a single probe request is allowed through.
    HalfOpen,
}

impl CircuitBreaker {
    /// Create a new circuit breaker.
    ///
    /// * `failure_threshold` — consecutive failures before the breaker opens.
    /// * `cooldown` — time to wait before transitioning from Open to HalfOpen.
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            failure_count: 0,
            failure_threshold,
            last_failure: None,
            cooldown,
            state: CircuitState::Closed,
        }
    }

    /// Record a successful operation — resets the breaker to Closed.
    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.last_failure = None;
        self.state = CircuitState::Closed;
    }

    /// Record a failed operation.
    ///
    /// Increments the failure counter and transitions to Open once the
    /// threshold is reached.
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());
        if self.failure_count >= self.failure_threshold {
            self.state = CircuitState::Open;
        }
    }

    /// Returns `true` when requests may be sent to the provider.
    ///
    /// * `Closed` — always available.
    /// * `Open` — unavailable unless the cooldown has elapsed, in which case
    ///   the state transitions to `HalfOpen` and we return `true`.
    /// * `HalfOpen` — available (one probe request).
    pub fn is_available(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(last) = self.last_failure {
                    if last.elapsed() >= self.cooldown {
                        self.state = CircuitState::HalfOpen;
                        return true;
                    }
                }
                false
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Provider entry (internal)
// ---------------------------------------------------------------------------

/// A named provider paired with its circuit breaker and priority.
struct ProviderEntry {
    name: String,
    provider: Arc<dyn StorageProvider>,
    circuit_breaker: Mutex<CircuitBreaker>,
    priority: u32, // lower = higher priority
}

// ---------------------------------------------------------------------------
// Failover metrics
// ---------------------------------------------------------------------------

/// Counters tracking failover behaviour.
#[derive(Debug, Clone, Default)]
pub struct FailoverMetrics {
    /// Total requests routed through the failover layer.
    pub total_requests: u64,
    /// Requests that succeeded on the highest-priority available provider.
    pub primary_successes: u64,
    /// Requests that required falling over to a lower-priority provider.
    pub failover_count: u64,
    /// Requests where every provider failed.
    pub total_failures: u64,
    /// Number of integrity checks performed.
    pub integrity_checks: u64,
    /// Integrity checks that detected corruption.
    pub integrity_failures: u64,
    /// Cumulative store latency in milliseconds.
    pub store_latency_sum_ms: u64,
    /// Number of completed store operations (for computing average latency).
    pub store_count: u64,
}

// ---------------------------------------------------------------------------
// FailoverRouter
// ---------------------------------------------------------------------------

/// Routes storage operations across multiple providers using priority-ordered
/// failover and circuit breakers.
pub struct FailoverRouter {
    providers: Vec<ProviderEntry>,
    failure_threshold: u32,
    cooldown: Duration,
    metrics: Mutex<FailoverMetrics>,
}

impl FailoverRouter {
    /// Create an empty router.
    ///
    /// * `failure_threshold` — consecutive failures before a provider's
    ///   circuit breaker opens.
    /// * `cooldown` — time a tripped provider must wait before receiving a
    ///   probe request.
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            providers: Vec::new(),
            failure_threshold,
            cooldown,
            metrics: Mutex::new(FailoverMetrics::default()),
        }
    }

    /// Register a provider. Lower `priority` values are tried first.
    pub fn add_provider(&mut self, name: &str, provider: Arc<dyn StorageProvider>, priority: u32) {
        self.providers.push(ProviderEntry {
            name: name.to_string(),
            provider,
            circuit_breaker: Mutex::new(CircuitBreaker::new(self.failure_threshold, self.cooldown)),
            priority,
        });
        // Keep sorted by priority (stable — insertion order breaks ties).
        self.providers.sort_by_key(|e| e.priority);
    }

    /// Store data, trying providers in priority order.
    ///
    /// Returns the [`ContentId`] from the first provider that succeeds.
    pub async fn store(&self, data: &[u8], metadata: &Metadata) -> Result<ContentId, StorageError> {
        let store_start = Instant::now();
        {
            let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
            m.total_requests += 1;
        }

        let mut last_err: Option<StorageError> = None;
        let mut is_first_available = true;

        for entry in &self.providers {
            // Check circuit breaker availability.
            {
                let mut cb = entry
                    .circuit_breaker
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if !cb.is_available() {
                    continue;
                }
            }

            match entry.provider.store(data, metadata).await {
                Ok(cid) => {
                    {
                        let mut cb = entry
                            .circuit_breaker
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        cb.record_success();
                    }
                    {
                        let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
                        if is_first_available {
                            m.primary_successes += 1;
                        } else {
                            m.failover_count += 1;
                        }
                        m.store_latency_sum_ms += store_start.elapsed().as_millis() as u64;
                        m.store_count += 1;
                    }
                    return Ok(cid);
                }
                Err(e) => {
                    {
                        let mut cb = entry
                            .circuit_breaker
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        cb.record_failure();
                    }
                    last_err = Some(e);
                    is_first_available = false;
                }
            }
        }

        {
            let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
            m.total_failures += 1;
        }

        Err(last_err
            .unwrap_or_else(|| StorageError::ProviderError("no providers available".into())))
    }

    /// Retrieve data with optional integrity verification.
    ///
    /// If `expected_hash` is provided the retrieved bytes are verified against
    /// it using [`IntegrityVerifier`]. A mismatch causes the router to try the
    /// next available provider.
    pub async fn retrieve(
        &self,
        id: &ContentId,
        expected_hash: Option<&str>,
    ) -> Result<Vec<u8>, StorageError> {
        {
            let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
            m.total_requests += 1;
        }

        let mut last_err: Option<StorageError> = None;
        let mut is_first_available = true;

        for entry in &self.providers {
            {
                let mut cb = entry
                    .circuit_breaker
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if !cb.is_available() {
                    continue;
                }
            }

            match entry.provider.retrieve(id).await {
                Ok(data) => {
                    // Integrity check if a hash was supplied.
                    if let Some(hash) = expected_hash {
                        let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
                        m.integrity_checks += 1;
                        if !IntegrityVerifier::verify(&data, hash) {
                            m.integrity_failures += 1;
                            drop(m);
                            {
                                let mut cb = entry
                                    .circuit_breaker
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner());
                                cb.record_failure();
                            }
                            last_err = Some(StorageError::InvalidData(format!(
                                "integrity mismatch from provider {}",
                                entry.name
                            )));
                            is_first_available = false;
                            continue;
                        }
                    }

                    {
                        let mut cb = entry
                            .circuit_breaker
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        cb.record_success();
                    }
                    {
                        let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
                        if is_first_available {
                            m.primary_successes += 1;
                        } else {
                            m.failover_count += 1;
                        }
                    }
                    return Ok(data);
                }
                Err(e) => {
                    {
                        let mut cb = entry
                            .circuit_breaker
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        cb.record_failure();
                    }
                    last_err = Some(e);
                    is_first_available = false;
                }
            }
        }

        {
            let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
            m.total_failures += 1;
        }

        Err(last_err
            .unwrap_or_else(|| StorageError::ProviderError("no providers available".into())))
    }

    /// Check whether content exists on any available provider.
    pub async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
        for entry in &self.providers {
            {
                let mut cb = entry
                    .circuit_breaker
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if !cb.is_available() {
                    continue;
                }
            }

            match entry.provider.exists(id).await {
                Ok(true) => {
                    let mut cb = entry
                        .circuit_breaker
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    cb.record_success();
                    return Ok(true);
                }
                Ok(false) => {
                    let mut cb = entry
                        .circuit_breaker
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    cb.record_success();
                    // Content might exist on another provider.
                }
                Err(_) => {
                    let mut cb = entry
                        .circuit_breaker
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    cb.record_failure();
                }
            }
        }

        Ok(false)
    }

    /// Query the health of every registered provider.
    pub async fn health(&self) -> Vec<(String, Result<HealthStatus, StorageError>)> {
        let mut results = Vec::with_capacity(self.providers.len());
        for entry in &self.providers {
            let result = entry.provider.health_check().await;
            results.push((entry.name.clone(), result));
        }
        results
    }

    /// Return a snapshot of the current failover metrics.
    pub fn metrics(&self) -> FailoverMetrics {
        self.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

// ---------------------------------------------------------------------------
// StorageProvider impl — allows FailoverRouter to be used as a single provider
// ---------------------------------------------------------------------------

#[async_trait]
impl StorageProvider for FailoverRouter {
    async fn store(&self, data: &[u8], metadata: &Metadata) -> Result<ContentId, StorageError> {
        FailoverRouter::store(self, data, metadata).await
    }

    /// NOTE: The `StorageProvider` trait `retrieve` does not perform integrity
    /// verification because the trait signature has no `expected_hash` parameter.
    /// For integrity-verified retrieval, use `FailoverRouter::retrieve()` directly
    /// with an `expected_hash`.
    async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        FailoverRouter::retrieve(self, id, None).await
    }

    async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
        FailoverRouter::exists(self, id).await
    }

    async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
        let mut last_err: Option<StorageError> = None;
        for entry in &self.providers {
            {
                let mut cb = entry
                    .circuit_breaker
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if !cb.is_available() {
                    continue;
                }
            }
            match entry.provider.delete(id).await {
                Ok(()) => {
                    let mut cb = entry
                        .circuit_breaker
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    cb.record_success();
                    return Ok(());
                }
                Err(e) => {
                    let mut cb = entry
                        .circuit_breaker
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    cb.record_failure();
                    last_err = Some(e);
                }
            }
        }
        Err(last_err
            .unwrap_or_else(|| StorageError::ProviderError("no providers available".into())))
    }

    async fn health_check(&self) -> Result<HealthStatus, StorageError> {
        let results = FailoverRouter::health(self).await;
        let healthy_count = results.iter().filter(|(_, r)| r.is_ok()).count();
        let total = results.len();
        Ok(HealthStatus {
            healthy: healthy_count > 0,
            provider: "failover-router".to_string(),
            latency_ms: None,
            message: Some(format!("{healthy_count}/{total} providers healthy")),
        })
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let mut agg = StorageStats::default();
        for entry in &self.providers {
            if let Ok(s) = entry.provider.stats().await {
                agg.total_stored += s.total_stored;
                agg.total_retrieved += s.total_retrieved;
                agg.total_bytes += s.total_bytes;
                agg.cache_hits += s.cache_hits;
                agg.cache_misses += s.cache_misses;
            }
        }
        Ok(agg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_breaker_starts_closed() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(10));
        assert_eq!(cb.state, CircuitState::Closed);
        assert_eq!(cb.failure_count, 0);
    }

    #[test]
    fn circuit_breaker_opens_after_threshold() {
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(10));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Open);
        assert!(!cb.is_available());
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(10));
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.state, CircuitState::Closed);
        assert_eq!(cb.failure_count, 0);
    }

    #[test]
    fn circuit_breaker_half_open_after_cooldown() {
        let mut cb = CircuitBreaker::new(2, Duration::from_millis(0));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Open);
        // Cooldown is 0ms so it should immediately become HalfOpen.
        assert!(cb.is_available());
        assert_eq!(cb.state, CircuitState::HalfOpen);
    }
}
