//! Periodic retention policy enforcement scheduler.
//!
//! Wraps a [`RetentionManager`] and calls
//! [`check_and_enforce`](RetentionManager::check_and_enforce) on a configurable
//! interval, logging every violation and recording the latest report for callers
//! to inspect.
//!
//! ## Shutdown
//!
//! Graceful shutdown is signalled via a [`tokio::sync::watch`] channel.  The
//! caller creates the channel, passes the receiver to [`RetentionScheduler::spawn`],
//! and sends `true` when it wants the loop to stop.
//!
//! ```no_run
//! # use std::{sync::{Arc, Mutex}, time::Duration};
//! # use wrfcoin_data_tiering::{RetentionManager, retention_scheduler::{RetentionScheduler, RetentionSchedulerConfig, ContentSource}};
//! # use wrfcoin_data_tiering::retention::ContentItem;
//! # use async_trait::async_trait;
//! struct NoopSource;
//! #[async_trait]
//! impl ContentSource for NoopSource {
//!     async fn get_content_items(&self) -> Vec<ContentItem> { vec![] }
//! }
//!
//! # async fn example() {
//! let manager = RetentionManager::with_environmental_defaults();
//! let scheduler = RetentionScheduler::new(manager, RetentionSchedulerConfig::default());
//! let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
//! let handle = scheduler.spawn(Arc::new(NoopSource), shutdown_rx);
//!
//! // ... later:
//! let _ = shutdown_tx.send(true);
//! handle.await.unwrap();
//! # }
//! ```

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tokio::{sync::watch, task::JoinHandle, time};

use crate::retention::{ContentItem, EnforcementReport, RetentionManager};

// ── Public interface ──────────────────────────────────────────────────────────

/// Provides a snapshot of content items to enforce retention against.
///
/// Implement this trait to bridge the scheduler to your actual storage backend
/// (Filecoin `DealManager`, a database query, etc.).
#[async_trait]
pub trait ContentSource: Send + Sync + 'static {
    async fn get_content_items(&self) -> Vec<ContentItem>;
}

/// Configuration for [`RetentionScheduler`].
#[derive(Debug, Clone)]
pub struct RetentionSchedulerConfig {
    /// How often the enforcement check runs.  Default: 1 hour.
    pub interval: Duration,
}

impl Default for RetentionSchedulerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(3600),
        }
    }
}

/// Mutable state tracked across enforcement runs.
#[derive(Debug, Default)]
struct SchedulerState {
    last_run_at: Option<Instant>,
    last_report: Option<EnforcementReport>,
    run_count: u64,
}

/// Periodic retention policy enforcement scheduler.
///
/// Create with [`RetentionScheduler::new`], then call [`spawn`](Self::spawn) to
/// start the background loop.  The scheduler itself remains usable for
/// querying state while the loop is running.
pub struct RetentionScheduler {
    manager: Arc<Mutex<RetentionManager>>,
    config: RetentionSchedulerConfig,
    state: Arc<Mutex<SchedulerState>>,
}

impl RetentionScheduler {
    /// Create a new scheduler wrapping the given manager.
    pub fn new(manager: RetentionManager, config: RetentionSchedulerConfig) -> Self {
        Self {
            manager: Arc::new(Mutex::new(manager)),
            config,
            state: Arc::new(Mutex::new(SchedulerState::default())),
        }
    }

    /// Spawn the periodic enforcement loop.
    ///
    /// The loop runs until `shutdown_rx` receives `true`.  The returned
    /// `JoinHandle` resolves when the loop exits cleanly.
    ///
    /// `spawn` does **not** consume `self` — the scheduler remains available
    /// for [`last_run_at`](Self::last_run_at), [`last_report`](Self::last_report),
    /// and [`run_count`](Self::run_count) queries.
    pub fn spawn(
        &self,
        source: Arc<dyn ContentSource>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        let manager = Arc::clone(&self.manager);
        let state = Arc::clone(&self.state);
        let interval = self.config.interval;

        tokio::spawn(run_loop(manager, interval, shutdown_rx, state, source))
    }

    /// Timestamp of the last completed enforcement run, if any.
    pub fn last_run_at(&self) -> Option<Instant> {
        self.state.lock().unwrap().last_run_at
    }

    /// The most recent enforcement report, if any.
    pub fn last_report(&self) -> Option<EnforcementReport> {
        self.state.lock().unwrap().last_report.clone()
    }

    /// Total number of enforcement runs completed since the scheduler was created.
    pub fn run_count(&self) -> u64 {
        self.state.lock().unwrap().run_count
    }
}

// ── Internal loop ─────────────────────────────────────────────────────────────

async fn run_loop(
    manager: Arc<Mutex<RetentionManager>>,
    check_interval: Duration,
    mut shutdown_rx: watch::Receiver<bool>,
    state: Arc<Mutex<SchedulerState>>,
    source: Arc<dyn ContentSource>,
) {
    let mut ticker = time::interval(check_interval);
    // Consume the first tick that fires immediately so the first real check
    // happens after one full interval.
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let items = source.get_content_items().await;
                let item_count = items.len();

                // Hold the lock only for the synchronous enforcement call —
                // never across an await point.
                let report = {
                    let mgr = manager.lock().unwrap();
                    mgr.check_and_enforce(&items)
                };

                let violation_count = report.violations.len();
                log_report(&report);

                {
                    let mut s = state.lock().unwrap();
                    s.last_run_at = Some(Instant::now());
                    s.last_report = Some(report);
                    s.run_count += 1;
                }

                tracing_log(item_count, violation_count);
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
        }
    }
}

/// Log individual violations to eprintln (no tracing dependency required).
fn log_report(report: &EnforcementReport) {
    for v in &report.violations {
        match &v.kind {
            crate::retention::ViolationKind::Expired { age_secs, max_age_secs } => {
                eprintln!(
                    "[RetentionScheduler] EXPIRED  {} ({}) age={}s max={}s",
                    v.content_id, v.artifact_class, age_secs, max_age_secs
                );
            }
            crate::retention::ViolationKind::UnderReplicated { have, need } => {
                eprintln!(
                    "[RetentionScheduler] UNDER_REPLICATED  {} ({}) have={} need={}",
                    v.content_id, v.artifact_class, have, need
                );
            }
            crate::retention::ViolationKind::BelowQuality { score, min_score } => {
                eprintln!(
                    "[RetentionScheduler] BELOW_QUALITY  {} ({}) score={:.2} min={:.2}",
                    v.content_id, v.artifact_class, score, min_score
                );
            }
        }
    }
}

fn tracing_log(items: usize, violations: usize) {
    eprintln!(
        "[RetentionScheduler] enforcement run complete: {} items, {} violations",
        items, violations
    );
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retention::ContentItem;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wrfcoin_storage_common::types::{DataTier, RetentionPolicy};

    // Minimal content source that returns a fixed item list.
    struct FixedSource {
        items: Vec<ContentItem>,
        call_count: Arc<AtomicUsize>,
    }

    impl FixedSource {
        fn new(items: Vec<ContentItem>) -> Self {
            Self {
                items,
                call_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn call_count(&self) -> Arc<AtomicUsize> {
            Arc::clone(&self.call_count)
        }
    }

    #[async_trait]
    impl ContentSource for FixedSource {
        async fn get_content_items(&self) -> Vec<ContentItem> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            self.items.clone()
        }
    }

    fn expired_item() -> ContentItem {
        ContentItem {
            content_id: "cid-1".into(),
            artifact_class: "forecast".into(),
            tier: DataTier::Archive,
            age_secs: 7200,   // 2 hours
            quality_score: 0.9,
            replica_count: 2,
        }
    }

    fn policy_1h() -> RetentionPolicy {
        let mut p = RetentionPolicy::new("forecast", DataTier::Archive, 2);
        p.max_age_secs = 3600;
        p
    }

    // ── unit tests for check_and_enforce (sync) ───────────────────────────────

    #[test]
    fn expired_item_produces_violation() {
        let mut mgr = RetentionManager::new();
        mgr.add_policy(policy_1h());

        let report = mgr.check_and_enforce(&[expired_item()]);
        assert_eq!(report.items_checked, 1);
        assert_eq!(report.expired_count(), 1);
    }

    #[test]
    fn under_replicated_item_produces_violation() {
        let mut mgr = RetentionManager::new();
        // Requires 5 copies
        mgr.add_policy(RetentionPolicy::new("forecast", DataTier::Archive, 5));

        let item = ContentItem {
            content_id: "cid-2".into(),
            artifact_class: "forecast".into(),
            tier: DataTier::Archive,
            age_secs: 100,
            quality_score: 0.9,
            replica_count: 2,
        };

        let report = mgr.check_and_enforce(&[item]);
        assert_eq!(report.under_replicated_count(), 1);
    }

    #[test]
    fn item_with_no_matching_policy_produces_no_violation() {
        let mgr = RetentionManager::new();
        let report = mgr.check_and_enforce(&[expired_item()]);
        assert!(report.violations.is_empty());
    }

    #[test]
    fn fresh_item_within_policy_produces_no_violation() {
        let mut mgr = RetentionManager::new();
        mgr.add_policy(policy_1h());

        let fresh = ContentItem {
            content_id: "cid-3".into(),
            artifact_class: "forecast".into(),
            tier: DataTier::Archive,
            age_secs: 600,    // 10 min — within 1h max
            quality_score: 0.9,
            replica_count: 3,
        };

        let report = mgr.check_and_enforce(&[fresh]);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    // ── async scheduler tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn scheduler_runs_once_and_records_report() {
        let mut mgr = RetentionManager::new();
        mgr.add_policy(policy_1h());

        let config = RetentionSchedulerConfig {
            interval: Duration::from_millis(50),
        };
        let scheduler = RetentionScheduler::new(mgr, config);

        let source = FixedSource::new(vec![expired_item()]);
        let call_count = source.call_count();

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = scheduler.spawn(Arc::new(source), shutdown_rx);

        // Wait for at least one enforcement run.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if scheduler.run_count() >= 1 {
                break;
            }
            assert!(Instant::now() < deadline, "scheduler did not run within 2s");
        }

        let _ = shutdown_tx.send(true);
        handle.await.unwrap();

        assert!(scheduler.run_count() >= 1);
        assert!(call_count.load(Ordering::Relaxed) >= 1);

        let report = scheduler.last_report().expect("report should be set");
        assert_eq!(report.items_checked, 1);
        assert_eq!(report.expired_count(), 1);
    }

    #[tokio::test]
    async fn scheduler_shuts_down_cleanly() {
        let mgr = RetentionManager::new();
        let config = RetentionSchedulerConfig {
            interval: Duration::from_millis(50),
        };
        let scheduler = RetentionScheduler::new(mgr, config);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = scheduler.spawn(Arc::new(FixedSource::new(vec![])), shutdown_rx);

        // Give it one tick then shut down.
        tokio::time::sleep(Duration::from_millis(80)).await;
        let _ = shutdown_tx.send(true);

        // Should resolve without hanging.
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("scheduler did not shut down within 2s")
            .unwrap();
    }
}
