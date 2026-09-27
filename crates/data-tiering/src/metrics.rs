//! Trait-based metrics collection for archive pipeline operations.

use std::sync::atomic::{AtomicU64, Ordering};

/// Trait for archive pipeline metrics collection.
pub trait MetricsCollector: Send + Sync {
    /// Record an archive attempt for the named provider.
    fn record_archive_attempt(&self, provider: &str, success: bool);
    /// Record a retrieval from the named provider.
    fn record_retrieval(&self, provider: &str, success: bool, latency_ms: u64);
    /// Record an integrity check result.
    fn record_integrity_check(&self, provider: &str, passed: bool);
}

/// In-memory metrics collector for testing and local use.
pub struct InMemoryMetrics {
    pub archive_attempts: AtomicU64,
    pub archive_successes: AtomicU64,
    pub archive_failures: AtomicU64,
    pub retrieval_attempts: AtomicU64,
    pub retrieval_successes: AtomicU64,
    pub retrieval_failures: AtomicU64,
    pub integrity_passes: AtomicU64,
    pub integrity_failures: AtomicU64,
    pub total_retrieval_latency_ms: AtomicU64,
}

impl InMemoryMetrics {
    /// Create a new zeroed in-memory metrics collector.
    pub fn new() -> Self {
        Self {
            archive_attempts: AtomicU64::new(0),
            archive_successes: AtomicU64::new(0),
            archive_failures: AtomicU64::new(0),
            retrieval_attempts: AtomicU64::new(0),
            retrieval_successes: AtomicU64::new(0),
            retrieval_failures: AtomicU64::new(0),
            integrity_passes: AtomicU64::new(0),
            integrity_failures: AtomicU64::new(0),
            total_retrieval_latency_ms: AtomicU64::new(0),
        }
    }
}

impl Default for InMemoryMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector for InMemoryMetrics {
    fn record_archive_attempt(&self, _provider: &str, success: bool) {
        self.archive_attempts.fetch_add(1, Ordering::Relaxed);
        if success {
            self.archive_successes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.archive_failures.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_retrieval(&self, _provider: &str, success: bool, latency_ms: u64) {
        self.retrieval_attempts.fetch_add(1, Ordering::Relaxed);
        if success {
            self.retrieval_successes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.retrieval_failures.fetch_add(1, Ordering::Relaxed);
        }
        self.total_retrieval_latency_ms
            .fetch_add(latency_ms, Ordering::Relaxed);
    }

    fn record_integrity_check(&self, _provider: &str, passed: bool) {
        if passed {
            self.integrity_passes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.integrity_failures.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_metrics_new() {
        let m = InMemoryMetrics::new();
        assert_eq!(m.archive_attempts.load(Ordering::Relaxed), 0);
        assert_eq!(m.archive_successes.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_record_archive_success() {
        let m = InMemoryMetrics::new();
        m.record_archive_attempt("ipfs", true);
        assert_eq!(m.archive_attempts.load(Ordering::Relaxed), 1);
        assert_eq!(m.archive_successes.load(Ordering::Relaxed), 1);
        assert_eq!(m.archive_failures.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_record_archive_failure() {
        let m = InMemoryMetrics::new();
        m.record_archive_attempt("ipfs", false);
        assert_eq!(m.archive_attempts.load(Ordering::Relaxed), 1);
        assert_eq!(m.archive_successes.load(Ordering::Relaxed), 0);
        assert_eq!(m.archive_failures.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_record_retrieval() {
        let m = InMemoryMetrics::new();
        m.record_retrieval("ipfs", true, 42);
        assert_eq!(m.retrieval_attempts.load(Ordering::Relaxed), 1);
        assert_eq!(m.retrieval_successes.load(Ordering::Relaxed), 1);
        assert_eq!(m.total_retrieval_latency_ms.load(Ordering::Relaxed), 42);
    }

    #[test]
    fn test_record_integrity_check() {
        let m = InMemoryMetrics::new();
        m.record_integrity_check("ipfs", true);
        m.record_integrity_check("filecoin", false);
        assert_eq!(m.integrity_passes.load(Ordering::Relaxed), 1);
        assert_eq!(m.integrity_failures.load(Ordering::Relaxed), 1);
    }
}
