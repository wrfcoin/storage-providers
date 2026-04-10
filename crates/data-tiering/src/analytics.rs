//! Storage analytics snapshot and export layer.
//!
//! [`StorageAnalytics`] aggregates metrics from all storage subsystems into a
//! single serializable [`StorageSnapshot`]. Two export formats are provided:
//!
//! - **JSON** — for the backend REST service (future HTTP endpoint, issue #31)
//! - **Prometheus text** — for `infra/monitoring/` Prometheus scraping
//!
//! # Usage
//!
//! ```rust,ignore
//! let mut analytics = StorageAnalytics::new(60); // keep 60 snapshots
//! let snap = analytics.snapshot(&AnalyticsInput { ... });
//! let json = StorageAnalytics::export_json(&snap);
//! let prom = StorageAnalytics::export_prometheus(&snap);
//! ```

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use wrfcoin_storage_common::{
    AvailabilitySlaStatus, DataTier, DomainContentIndex, EnvironmentalDomain, FailoverMetrics,
    StorageStats,
};

use crate::budget::StorageBudget;
use crate::metrics::InMemoryMetrics;
use crate::tiers::TieringConfig;

// ---------------------------------------------------------------------------
// Snapshot structs — all derive Serialize for JSON export
// ---------------------------------------------------------------------------

/// Top-level analytics snapshot for a single point in time.
#[derive(Debug, Clone, Serialize)]
pub struct StorageSnapshot {
    /// Unix timestamp (seconds) when this snapshot was taken.
    pub timestamp: u64,
    /// Per-provider health and traffic metrics.
    pub providers: Vec<ProviderSnapshot>,
    /// Per-tier cost, utilization, and budget metrics.
    pub tiers: Vec<TierSnapshot>,
    /// Per-environmental-domain object counts.
    pub domains: Vec<DomainSnapshot>,
    /// Aggregated budget summary.
    pub budget: BudgetSnapshot,
    /// SLA compliance status for each report class.
    pub sla_compliance: Vec<SlaSnapshot>,
    /// Aggregated failover-layer metrics.
    pub failover: FailoverSnapshot,
}

/// Metrics for a single storage provider.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderSnapshot {
    /// Provider name (e.g. "ipfs", "filecoin", "arweave").
    pub name: String,
    /// Whether the provider passed its most recent health check.
    pub healthy: bool,
    /// Successful archive (store) operations recorded so far.
    pub objects_stored: u64,
    /// Total bytes written to this provider.
    pub bytes_stored: u64,
    /// Average retrieval latency in milliseconds (0.0 if no retrievals yet).
    pub avg_retrieval_latency_ms: f64,
    /// Fraction of archive attempts that failed  (0.0–1.0; 0.0 if no attempts).
    pub error_rate: f64,
    /// Cumulative cost charged against this provider, in USD.
    pub cost_to_date: f64,
}

/// Cost, utilization, and budget metrics for a single tier.
#[derive(Debug, Clone, Serialize)]
pub struct TierSnapshot {
    /// Tier name: "Hot", "Warm", "Cold", or "Archive".
    pub tier: String,
    /// Bytes currently stored in this tier.
    /// Populated by the caller via [`TierBytes`]; defaults to 0 when not provided.
    pub bytes_stored: u64,
    /// Cumulative spend against this tier's budget, in USD.
    pub cost_to_date: f64,
    /// Remaining budget for this tier in USD (f64::INFINITY when no limit is set).
    pub budget_remaining: f64,
    /// Configured cost per GiB for this tier, in USD.
    pub cost_per_gib: f64,
    /// Maximum tier capacity in bytes (0 = unlimited, as for Archive).
    pub max_size_bytes: u64,
    /// Utilization as a percentage of `max_size_bytes` (0.0 when unlimited).
    pub utilization_pct: f64,
}

/// Object count for a single environmental domain.
#[derive(Debug, Clone, Serialize)]
pub struct DomainSnapshot {
    /// Domain name (e.g. "Weather", "AirQuality").
    pub domain: String,
    /// Number of objects indexed for this domain.
    pub object_count: u64,
    /// Bytes stored for this domain.
    /// Populated by the caller via [`DomainBytes`]; defaults to 0 when not provided.
    pub bytes_stored: u64,
}

/// Aggregated budget summary across all tiers.
#[derive(Debug, Clone, Serialize)]
pub struct BudgetSnapshot {
    /// Total spend across all tiers in USD.
    pub total_spend: f64,
    /// Per-tier breakdown of spend and remaining budget.
    pub per_tier: Vec<TierBudgetEntry>,
}

/// Per-tier budget entry inside [`BudgetSnapshot`].
#[derive(Debug, Clone, Serialize)]
pub struct TierBudgetEntry {
    /// Tier name.
    pub tier: String,
    /// Current spend in USD.
    pub spend: f64,
    /// Remaining budget in USD (f64::INFINITY when no limit is set).
    pub remaining: f64,
}

/// SLA compliance for a single report class.
#[derive(Debug, Clone, Serialize)]
pub struct SlaSnapshot {
    /// Report class identifier.
    pub class: String,
    /// Target replica count required by the policy.
    pub target_replicas: u8,
    /// Actual replica count at snapshot time.
    pub actual_replicas: u8,
    /// Whether the failover layer has at least one secondary provider ready.
    pub read_failover_ready: bool,
    /// `true` when `actual_replicas >= target_replicas`.
    pub sla_met: bool,
}

/// Aggregated failover-layer metrics.
#[derive(Debug, Clone, Serialize)]
pub struct FailoverSnapshot {
    /// Total requests routed through the failover layer.
    pub total_requests: u64,
    /// Requests that succeeded on the primary (highest-priority) provider.
    pub primary_successes: u64,
    /// Requests that required falling over to a lower-priority provider.
    pub failover_count: u64,
    /// Requests where every provider failed.
    pub total_failures: u64,
    /// Integrity checks performed.
    pub integrity_checks: u64,
    /// Integrity checks that detected corruption.
    pub integrity_failures: u64,
    /// Average store latency in milliseconds (0.0 if no stores yet).
    pub avg_store_latency_ms: f64,
    /// Fraction of requests served by the primary provider (0.0–1.0).
    pub primary_success_rate: f64,
}

// ---------------------------------------------------------------------------
// Optional caller-supplied byte-tracking maps
// ---------------------------------------------------------------------------

/// Caller-supplied bytes-per-tier map for populating [`TierSnapshot::bytes_stored`].
pub type TierBytes<'a> = &'a std::collections::HashMap<DataTier, u64>;

/// Caller-supplied bytes-per-domain map for populating [`DomainSnapshot::bytes_stored`].
pub type DomainBytes<'a> = &'a std::collections::HashMap<EnvironmentalDomain, u64>;

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Metrics snapshot for a single storage provider.
pub struct ProviderInput<'a> {
    /// Provider name (e.g. "ipfs", "filecoin", "arweave").
    pub name: &'a str,
    /// Whether the provider is currently healthy.
    pub healthy: bool,
    /// Aggregate byte and operation counters.
    pub stats: &'a StorageStats,
    /// In-process atomic archive/retrieval/integrity counters.
    pub metrics: &'a InMemoryMetrics,
    /// Cumulative cost charged against this provider in USD.
    pub cost_to_date: f64,
}

/// All inputs required to build a [`StorageSnapshot`].
pub struct AnalyticsInput<'a> {
    /// One entry per storage provider.
    pub providers: Vec<ProviderInput<'a>>,
    /// Current budget state (spend and limits per tier).
    pub budget: &'a StorageBudget,
    /// Tiering configuration (cost/GiB, max sizes).
    pub tiering_config: &'a TieringConfig,
    /// Domain content index for per-domain object counts.
    pub domain_index: &'a DomainContentIndex,
    /// Current SLA status per report class from the orchestrator.
    pub sla_statuses: &'a [AvailabilitySlaStatus],
    /// Failover router metrics.
    pub failover: &'a FailoverMetrics,
    /// Optional bytes-per-tier for [`TierSnapshot::bytes_stored`].
    /// Pass `None` when not tracked externally; the field defaults to 0.
    pub tier_bytes: Option<TierBytes<'a>>,
    /// Optional bytes-per-domain for [`DomainSnapshot::bytes_stored`].
    /// Pass `None` when not tracked externally; the field defaults to 0.
    pub domain_bytes: Option<DomainBytes<'a>>,
    /// Snapshot timestamp (Unix seconds). Pass 0 to use the current wall clock.
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// StorageAnalytics
// ---------------------------------------------------------------------------

/// Aggregates all storage metrics into serializable snapshots.
///
/// Optionally maintains a ring-buffer history of past snapshots for trend data.
/// The default history limit is 60 (one hour of per-minute snapshots).
pub struct StorageAnalytics {
    history: VecDeque<StorageSnapshot>,
    history_limit: usize,
}

/// All known `EnvironmentalDomain` variants in declaration order.
const ALL_DOMAINS: [EnvironmentalDomain; 7] = [
    EnvironmentalDomain::Weather,
    EnvironmentalDomain::AirQuality,
    EnvironmentalDomain::WaterQuality,
    EnvironmentalDomain::Wildfire,
    EnvironmentalDomain::Soil,
    EnvironmentalDomain::Ocean,
    EnvironmentalDomain::Hydrology,
];

/// All known `DataTier` variants in hotness order.
const ALL_TIERS: [DataTier; 4] = [DataTier::Hot, DataTier::Warm, DataTier::Cold, DataTier::Archive];

fn tier_name(tier: &DataTier) -> &'static str {
    match tier {
        DataTier::Hot => "Hot",
        DataTier::Warm => "Warm",
        DataTier::Cold => "Cold",
        DataTier::Archive => "Archive",
    }
}

impl StorageAnalytics {
    /// Create a new analytics collector with the given ring-buffer capacity.
    ///
    /// Set `history_limit` to 0 to disable snapshot history entirely.
    pub fn new(history_limit: usize) -> Self {
        Self {
            history: VecDeque::new(),
            history_limit,
        }
    }

    /// Build a [`StorageSnapshot`] from `input`, push it onto the ring buffer,
    /// and return it.
    pub fn snapshot(&mut self, input: &AnalyticsInput<'_>) -> StorageSnapshot {
        let timestamp = if input.timestamp != 0 {
            input.timestamp
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        };

        let snap = StorageSnapshot {
            timestamp,
            providers: Self::build_provider_snapshots(&input.providers),
            tiers: Self::build_tier_snapshots(input.budget, input.tiering_config, input.tier_bytes),
            domains: Self::build_domain_snapshots(input.domain_index, input.domain_bytes),
            budget: Self::build_budget_snapshot(input.budget),
            sla_compliance: Self::build_sla_snapshots(input.sla_statuses),
            failover: Self::build_failover_snapshot(input.failover),
        };

        if self.history_limit > 0 {
            if self.history.len() >= self.history_limit {
                self.history.pop_front();
            }
            self.history.push_back(snap.clone());
        }

        snap
    }

    /// Snapshot ring-buffer history, oldest entry first.
    pub fn history(&self) -> &VecDeque<StorageSnapshot> {
        &self.history
    }

    /// Serialize `snapshot` to a pretty-printed JSON string.
    pub fn export_json(snapshot: &StorageSnapshot) -> String {
        serde_json::to_string_pretty(snapshot)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
    }

    /// Serialize `snapshot` to [Prometheus text exposition format][prom].
    ///
    /// The output is suitable for the `infra/monitoring/` Prometheus instance
    /// to scrape directly. Each time-series carries the snapshot timestamp as
    /// the Prometheus sample timestamp (milliseconds).
    ///
    /// [prom]: https://prometheus.io/docs/instrumenting/exposition_formats/
    pub fn export_prometheus(snapshot: &StorageSnapshot) -> String {
        // Prometheus timestamps are milliseconds.
        let ts_ms = snapshot.timestamp * 1000;
        let mut out = String::with_capacity(4096);

        // ---- Provider metrics ----
        prom_comment(&mut out, "wrfcoin_storage_provider_objects_stored",
            "gauge", "Successful archive operations per provider");
        for p in &snapshot.providers {
            prom_line(&mut out, "wrfcoin_storage_provider_objects_stored",
                &[("provider", &p.name)], p.objects_stored as f64, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_provider_bytes_stored",
            "gauge", "Total bytes stored per provider");
        for p in &snapshot.providers {
            prom_line(&mut out, "wrfcoin_storage_provider_bytes_stored",
                &[("provider", &p.name)], p.bytes_stored as f64, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_provider_avg_retrieval_latency_ms",
            "gauge", "Average retrieval latency per provider in milliseconds");
        for p in &snapshot.providers {
            prom_line(&mut out, "wrfcoin_storage_provider_avg_retrieval_latency_ms",
                &[("provider", &p.name)], p.avg_retrieval_latency_ms, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_provider_error_rate",
            "gauge", "Fraction of archive attempts that failed per provider");
        for p in &snapshot.providers {
            prom_line(&mut out, "wrfcoin_storage_provider_error_rate",
                &[("provider", &p.name)], p.error_rate, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_provider_healthy",
            "gauge", "Provider health status (1=healthy, 0=unhealthy)");
        for p in &snapshot.providers {
            prom_line(&mut out, "wrfcoin_storage_provider_healthy",
                &[("provider", &p.name)], if p.healthy { 1.0 } else { 0.0 }, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_provider_cost_to_date_usd",
            "gauge", "Cumulative cost to date per provider in USD");
        for p in &snapshot.providers {
            prom_line(&mut out, "wrfcoin_storage_provider_cost_to_date_usd",
                &[("provider", &p.name)], p.cost_to_date, ts_ms);
        }

        // ---- Tier metrics ----
        prom_comment(&mut out, "wrfcoin_storage_tier_bytes_stored",
            "gauge", "Bytes stored per tier");
        for t in &snapshot.tiers {
            prom_line(&mut out, "wrfcoin_storage_tier_bytes_stored",
                &[("tier", &t.tier)], t.bytes_stored as f64, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_tier_cost_to_date_usd",
            "gauge", "Cumulative spend per tier in USD");
        for t in &snapshot.tiers {
            prom_line(&mut out, "wrfcoin_storage_tier_cost_to_date_usd",
                &[("tier", &t.tier)], t.cost_to_date, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_tier_budget_remaining_usd",
            "gauge", "Budget remaining per tier in USD (+Inf when unlimited)");
        for t in &snapshot.tiers {
            // Prometheus represents +Inf as the literal string "+Inf".
            let remaining_str = if t.budget_remaining.is_infinite() && t.budget_remaining > 0.0 {
                "+Inf".to_string()
            } else {
                t.budget_remaining.to_string()
            };
            out.push_str(&format!(
                "wrfcoin_storage_tier_budget_remaining_usd{{tier=\"{}\"}} {} {}\n",
                t.tier, remaining_str, ts_ms
            ));
        }

        prom_comment(&mut out, "wrfcoin_storage_tier_utilization_pct",
            "gauge", "Tier utilization as percentage of max_size_bytes (0 when unlimited)");
        for t in &snapshot.tiers {
            prom_line(&mut out, "wrfcoin_storage_tier_utilization_pct",
                &[("tier", &t.tier)], t.utilization_pct, ts_ms);
        }

        // ---- Domain metrics ----
        prom_comment(&mut out, "wrfcoin_storage_domain_objects_total",
            "gauge", "Objects stored per environmental domain");
        for d in &snapshot.domains {
            prom_line(&mut out, "wrfcoin_storage_domain_objects_total",
                &[("domain", &d.domain)], d.object_count as f64, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_domain_bytes_stored",
            "gauge", "Bytes stored per environmental domain");
        for d in &snapshot.domains {
            prom_line(&mut out, "wrfcoin_storage_domain_bytes_stored",
                &[("domain", &d.domain)], d.bytes_stored as f64, ts_ms);
        }

        // ---- Budget summary ----
        prom_comment(&mut out, "wrfcoin_storage_budget_total_spend_usd",
            "gauge", "Total spend across all tiers in USD");
        out.push_str(&format!(
            "wrfcoin_storage_budget_total_spend_usd {} {}\n",
            snapshot.budget.total_spend, ts_ms
        ));

        // ---- SLA metrics ----
        prom_comment(&mut out, "wrfcoin_storage_sla_met",
            "gauge", "SLA compliance per report class (1=met, 0=not met)");
        for s in &snapshot.sla_compliance {
            prom_line(&mut out, "wrfcoin_storage_sla_met",
                &[("class", &s.class)], if s.sla_met { 1.0 } else { 0.0 }, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_sla_actual_replicas",
            "gauge", "Actual replica count per report class");
        for s in &snapshot.sla_compliance {
            prom_line(&mut out, "wrfcoin_storage_sla_actual_replicas",
                &[("class", &s.class)], s.actual_replicas as f64, ts_ms);
        }

        prom_comment(&mut out, "wrfcoin_storage_sla_target_replicas",
            "gauge", "Target replica count per report class");
        for s in &snapshot.sla_compliance {
            prom_line(&mut out, "wrfcoin_storage_sla_target_replicas",
                &[("class", &s.class)], s.target_replicas as f64, ts_ms);
        }

        // ---- Failover metrics ----
        prom_comment(&mut out, "wrfcoin_storage_failover_requests_total",
            "counter", "Total requests routed through the failover layer");
        out.push_str(&format!(
            "wrfcoin_storage_failover_requests_total {} {}\n",
            snapshot.failover.total_requests, ts_ms
        ));

        prom_comment(&mut out, "wrfcoin_storage_failover_primary_successes_total",
            "counter", "Requests served by the primary provider");
        out.push_str(&format!(
            "wrfcoin_storage_failover_primary_successes_total {} {}\n",
            snapshot.failover.primary_successes, ts_ms
        ));

        prom_comment(&mut out, "wrfcoin_storage_failover_count_total",
            "counter", "Requests that required failover to a secondary provider");
        out.push_str(&format!(
            "wrfcoin_storage_failover_count_total {} {}\n",
            snapshot.failover.failover_count, ts_ms
        ));

        prom_comment(&mut out, "wrfcoin_storage_failover_failures_total",
            "counter", "Requests where every provider failed");
        out.push_str(&format!(
            "wrfcoin_storage_failover_failures_total {} {}\n",
            snapshot.failover.total_failures, ts_ms
        ));

        prom_comment(&mut out, "wrfcoin_storage_integrity_checks_total",
            "counter", "Integrity checks performed");
        out.push_str(&format!(
            "wrfcoin_storage_integrity_checks_total {} {}\n",
            snapshot.failover.integrity_checks, ts_ms
        ));

        prom_comment(&mut out, "wrfcoin_storage_integrity_failures_total",
            "counter", "Integrity checks that detected data corruption");
        out.push_str(&format!(
            "wrfcoin_storage_integrity_failures_total {} {}\n",
            snapshot.failover.integrity_failures, ts_ms
        ));

        prom_comment(&mut out, "wrfcoin_storage_primary_success_rate",
            "gauge", "Fraction of requests served by the primary provider (0.0–1.0)");
        out.push_str(&format!(
            "wrfcoin_storage_primary_success_rate {} {}\n",
            snapshot.failover.primary_success_rate, ts_ms
        ));

        prom_comment(&mut out, "wrfcoin_storage_avg_store_latency_ms",
            "gauge", "Average store latency across the failover layer in milliseconds");
        out.push_str(&format!(
            "wrfcoin_storage_avg_store_latency_ms {} {}\n",
            snapshot.failover.avg_store_latency_ms, ts_ms
        ));

        out
    }

    // ---------------------------------------------------------------------------
    // Private builder helpers
    // ---------------------------------------------------------------------------

    fn build_provider_snapshots(inputs: &[ProviderInput<'_>]) -> Vec<ProviderSnapshot> {
        inputs
            .iter()
            .map(|p| {
                let attempts = p.metrics.archive_attempts.load(Ordering::Relaxed);
                let successes = p.metrics.archive_successes.load(Ordering::Relaxed);
                let failures = p.metrics.archive_failures.load(Ordering::Relaxed);
                let retrievals = p.metrics.retrieval_attempts.load(Ordering::Relaxed);
                let latency_sum = p.metrics.total_retrieval_latency_ms.load(Ordering::Relaxed);

                ProviderSnapshot {
                    name: p.name.to_string(),
                    healthy: p.healthy,
                    objects_stored: successes,
                    bytes_stored: p.stats.total_bytes,
                    avg_retrieval_latency_ms: if retrievals > 0 {
                        latency_sum as f64 / retrievals as f64
                    } else {
                        0.0
                    },
                    error_rate: if attempts > 0 {
                        failures as f64 / attempts as f64
                    } else {
                        0.0
                    },
                    cost_to_date: p.cost_to_date,
                }
            })
            .collect()
    }

    fn build_tier_snapshots(
        budget: &StorageBudget,
        tiering_config: &TieringConfig,
        tier_bytes: Option<TierBytes<'_>>,
    ) -> Vec<TierSnapshot> {
        ALL_TIERS
            .iter()
            .map(|tier| {
                let config = tiering_config.config_for(tier);
                let spend = budget.tier_spend(tier);
                let remaining = budget.remaining(tier);
                let bytes_stored = tier_bytes
                    .and_then(|m| m.get(tier))
                    .copied()
                    .unwrap_or(0);

                let utilization_pct = if config.max_size_bytes > 0 {
                    (bytes_stored as f64 / config.max_size_bytes as f64 * 100.0).min(100.0)
                } else {
                    0.0 // unlimited tier (Archive)
                };

                TierSnapshot {
                    tier: tier_name(tier).to_string(),
                    bytes_stored,
                    cost_to_date: spend,
                    budget_remaining: remaining,
                    cost_per_gib: config.cost_per_gib,
                    max_size_bytes: config.max_size_bytes,
                    utilization_pct,
                }
            })
            .collect()
    }

    fn build_domain_snapshots(
        domain_index: &DomainContentIndex,
        domain_bytes: Option<DomainBytes<'_>>,
    ) -> Vec<DomainSnapshot> {
        ALL_DOMAINS
            .iter()
            .map(|domain| {
                let object_count = domain_index.count_by_domain(domain) as u64;
                let bytes_stored = domain_bytes
                    .and_then(|m| m.get(domain))
                    .copied()
                    .unwrap_or(0);
                DomainSnapshot {
                    domain: domain.to_string(),
                    object_count,
                    bytes_stored,
                }
            })
            .collect()
    }

    fn build_budget_snapshot(budget: &StorageBudget) -> BudgetSnapshot {
        BudgetSnapshot {
            total_spend: budget.total_spend(),
            per_tier: ALL_TIERS
                .iter()
                .map(|tier| TierBudgetEntry {
                    tier: tier_name(tier).to_string(),
                    spend: budget.tier_spend(tier),
                    remaining: budget.remaining(tier),
                })
                .collect(),
        }
    }

    fn build_sla_snapshots(statuses: &[AvailabilitySlaStatus]) -> Vec<SlaSnapshot> {
        statuses
            .iter()
            .map(|s| SlaSnapshot {
                class: s.class.clone(),
                target_replicas: s.target_replicas,
                actual_replicas: s.replicated_replicas,
                read_failover_ready: s.read_failover_ready,
                sla_met: s.sla_met,
            })
            .collect()
    }

    fn build_failover_snapshot(f: &FailoverMetrics) -> FailoverSnapshot {
        FailoverSnapshot {
            total_requests: f.total_requests,
            primary_successes: f.primary_successes,
            failover_count: f.failover_count,
            total_failures: f.total_failures,
            integrity_checks: f.integrity_checks,
            integrity_failures: f.integrity_failures,
            avg_store_latency_ms: if f.store_count > 0 {
                f.store_latency_sum_ms as f64 / f.store_count as f64
            } else {
                0.0
            },
            primary_success_rate: if f.total_requests > 0 {
                f.primary_successes as f64 / f.total_requests as f64
            } else {
                0.0
            },
        }
    }
}

impl Default for StorageAnalytics {
    fn default() -> Self {
        Self::new(60)
    }
}

// ---------------------------------------------------------------------------
// Prometheus formatting helpers
// ---------------------------------------------------------------------------

fn prom_comment(out: &mut String, name: &str, kind: &str, help: &str) {
    out.push_str(&format!("# HELP {name} {help}\n"));
    out.push_str(&format!("# TYPE {name} {kind}\n"));
}

fn prom_line(out: &mut String, name: &str, labels: &[(&str, &str)], value: f64, ts_ms: u64) {
    let label_str = labels
        .iter()
        .map(|(k, v)| format!("{k}=\"{v}\""))
        .collect::<Vec<_>>()
        .join(",");
    out.push_str(&format!("{name}{{{label_str}}} {value} {ts_ms}\n"));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use wrfcoin_storage_common::{AvailabilitySlaStatus, FailoverMetrics, StorageStats};
    use crate::metrics::MetricsCollector;

    fn empty_input<'a>(
        budget: &'a StorageBudget,
        tiering_config: &'a TieringConfig,
        domain_index: &'a DomainContentIndex,
        failover: &'a FailoverMetrics,
    ) -> AnalyticsInput<'a> {
        AnalyticsInput {
            providers: vec![],
            budget,
            tiering_config,
            domain_index,
            sla_statuses: &[],
            failover,
            tier_bytes: None,
            domain_bytes: None,
            timestamp: 1_000_000,
        }
    }

    // ---- Snapshot structure ----

    #[test]
    fn test_snapshot_timestamp_passthrough() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));
        assert_eq!(snap.timestamp, 1_000_000);
    }

    #[test]
    fn test_snapshot_uses_wall_clock_when_timestamp_zero() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let mut input = empty_input(&budget, &config, &index, &failover);
        input.timestamp = 0;
        let snap = analytics.snapshot(&input);
        // Wall clock must be a plausible Unix timestamp (after 2020-01-01).
        assert!(snap.timestamp > 1_577_836_800);
    }

    #[test]
    fn test_snapshot_has_all_four_tiers() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));
        assert_eq!(snap.tiers.len(), 4);
        let names: Vec<&str> = snap.tiers.iter().map(|t| t.tier.as_str()).collect();
        assert_eq!(names, ["Hot", "Warm", "Cold", "Archive"]);
    }

    #[test]
    fn test_snapshot_has_all_seven_domains() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));
        assert_eq!(snap.domains.len(), 7);
    }

    // ---- Provider snapshot ----

    #[test]
    fn test_provider_snapshot_computed_fields() {
        let stats = StorageStats {
            total_stored: 10,
            total_retrieved: 5,
            total_bytes: 1024,
            cache_hits: 2,
            cache_misses: 3,
        };
        let metrics = InMemoryMetrics::new();
        // 3 archive attempts: 2 succeed, 1 fails
        metrics.record_archive_attempt("ipfs", true);
        metrics.record_archive_attempt("ipfs", true);
        metrics.record_archive_attempt("ipfs", false);
        // 2 retrievals: latency 100ms and 200ms
        metrics.record_retrieval("ipfs", true, 100);
        metrics.record_retrieval("ipfs", true, 200);

        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let input = AnalyticsInput {
            providers: vec![ProviderInput {
                name: "ipfs",
                healthy: true,
                stats: &stats,
                metrics: &metrics,
                cost_to_date: 0.05,
            }],
            budget: &budget,
            tiering_config: &config,
            domain_index: &index,
            sla_statuses: &[],
            failover: &failover,
            tier_bytes: None,
            domain_bytes: None,
            timestamp: 1_000_000,
        };
        let snap = analytics.snapshot(&input);

        assert_eq!(snap.providers.len(), 1);
        let p = &snap.providers[0];
        assert_eq!(p.name, "ipfs");
        assert!(p.healthy);
        assert_eq!(p.objects_stored, 2); // archive_successes
        assert_eq!(p.bytes_stored, 1024);
        assert!((p.avg_retrieval_latency_ms - 150.0).abs() < f64::EPSILON);
        assert!((p.error_rate - 1.0 / 3.0).abs() < 1e-10);
        assert!((p.cost_to_date - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn test_provider_snapshot_zero_division_safety() {
        let stats = StorageStats::default();
        let metrics = InMemoryMetrics::new(); // all counters zero
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let input = AnalyticsInput {
            providers: vec![ProviderInput {
                name: "filecoin",
                healthy: false,
                stats: &stats,
                metrics: &metrics,
                cost_to_date: 0.0,
            }],
            budget: &budget,
            tiering_config: &config,
            domain_index: &index,
            sla_statuses: &[],
            failover: &failover,
            tier_bytes: None,
            domain_bytes: None,
            timestamp: 1_000_000,
        };
        let snap = analytics.snapshot(&input);
        let p = &snap.providers[0];
        assert_eq!(p.avg_retrieval_latency_ms, 0.0);
        assert_eq!(p.error_rate, 0.0);
    }

    // ---- Tier snapshot ----

    #[test]
    fn test_tier_snapshot_utilization_with_bytes() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();

        // Default hot tier max is 10 GiB = 10 * 1024^3 bytes
        let hot_max = config.config_for(&DataTier::Hot).max_size_bytes;
        // Fill it 50%
        let mut tier_map: HashMap<DataTier, u64> = HashMap::new();
        tier_map.insert(DataTier::Hot, hot_max / 2);

        let mut analytics = StorageAnalytics::new(10);
        let input = AnalyticsInput {
            providers: vec![],
            budget: &budget,
            tiering_config: &config,
            domain_index: &index,
            sla_statuses: &[],
            failover: &failover,
            tier_bytes: Some(&tier_map),
            domain_bytes: None,
            timestamp: 1_000_000,
        };
        let snap = analytics.snapshot(&input);

        let hot = snap.tiers.iter().find(|t| t.tier == "Hot").unwrap();
        assert_eq!(hot.bytes_stored, hot_max / 2);
        assert!((hot.utilization_pct - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_archive_tier_has_zero_utilization_when_unlimited() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));
        let archive = snap.tiers.iter().find(|t| t.tier == "Archive").unwrap();
        // Archive max_size_bytes is 0 (unlimited) → utilization_pct must be 0
        assert_eq!(archive.utilization_pct, 0.0);
    }

    #[test]
    fn test_tier_snapshot_budget_fields() {
        let mut budget = StorageBudget::new();
        budget.set_limit(DataTier::Hot, 100.0).unwrap();
        budget.record_cost(DataTier::Hot, 25.0).unwrap();

        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));

        let hot = snap.tiers.iter().find(|t| t.tier == "Hot").unwrap();
        assert!((hot.cost_to_date - 25.0).abs() < f64::EPSILON);
        assert!((hot.budget_remaining - 75.0).abs() < f64::EPSILON);
    }

    // ---- Domain snapshot ----

    #[test]
    fn test_domain_snapshot_counts_from_index() {
        use wrfcoin_storage_common::{ContentId, IndexEntry};

        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let failover = FailoverMetrics::default();
        let mut index = DomainContentIndex::new();

        // Insert 3 Weather and 1 Ocean entry
        for i in 0u32..3 {
            index.insert(IndexEntry {
                content_id: ContentId { provider: "ipfs".into(), hash: format!("hash-w{i}") },
                domain: EnvironmentalDomain::Weather,
                station_id: format!("stn-{i}"),
                report_id: format!("rpt-w{i}"),
                stored_at: 1_000_000,
            });
        }
        index.insert(IndexEntry {
            content_id: ContentId { provider: "ipfs".into(), hash: "hash-o1".into() },
            domain: EnvironmentalDomain::Ocean,
            station_id: "stn-o".into(),
            report_id: "rpt-o1".into(),
            stored_at: 1_000_000,
        });

        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));

        let weather = snap.domains.iter().find(|d| d.domain == "Weather").unwrap();
        assert_eq!(weather.object_count, 3);

        let ocean = snap.domains.iter().find(|d| d.domain == "Ocean").unwrap();
        assert_eq!(ocean.object_count, 1);

        let air = snap.domains.iter().find(|d| d.domain == "AirQuality").unwrap();
        assert_eq!(air.object_count, 0);
    }

    #[test]
    fn test_domain_snapshot_bytes_from_map() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();

        let mut domain_map: HashMap<EnvironmentalDomain, u64> = HashMap::new();
        domain_map.insert(EnvironmentalDomain::Weather, 4096);
        domain_map.insert(EnvironmentalDomain::Soil, 2048);

        let mut analytics = StorageAnalytics::new(10);
        let input = AnalyticsInput {
            providers: vec![],
            budget: &budget,
            tiering_config: &config,
            domain_index: &index,
            sla_statuses: &[],
            failover: &failover,
            tier_bytes: None,
            domain_bytes: Some(&domain_map),
            timestamp: 1_000_000,
        };
        let snap = analytics.snapshot(&input);

        let weather = snap.domains.iter().find(|d| d.domain == "Weather").unwrap();
        assert_eq!(weather.bytes_stored, 4096);

        let soil = snap.domains.iter().find(|d| d.domain == "Soil").unwrap();
        assert_eq!(soil.bytes_stored, 2048);

        let ocean = snap.domains.iter().find(|d| d.domain == "Ocean").unwrap();
        assert_eq!(ocean.bytes_stored, 0); // not in map
    }

    // ---- Budget snapshot ----

    #[test]
    fn test_budget_snapshot_total_spend() {
        let mut budget = StorageBudget::new();
        budget.record_cost(DataTier::Hot, 10.0).unwrap();
        budget.record_cost(DataTier::Warm, 5.0).unwrap();
        budget.record_cost(DataTier::Cold, 1.0).unwrap();

        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));

        assert!((snap.budget.total_spend - 16.0).abs() < f64::EPSILON);
        assert_eq!(snap.budget.per_tier.len(), 4);

        let hot = snap.budget.per_tier.iter().find(|t| t.tier == "Hot").unwrap();
        assert!((hot.spend - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_budget_remaining_is_infinite_when_no_limit() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));

        // No limits set → remaining should be +Inf
        for entry in &snap.budget.per_tier {
            assert!(
                entry.remaining.is_infinite() && entry.remaining > 0.0,
                "expected +Inf for {}, got {}",
                entry.tier,
                entry.remaining
            );
        }
    }

    // ---- SLA snapshot ----

    #[test]
    fn test_sla_snapshot_passthrough() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();

        let statuses = vec![
            AvailabilitySlaStatus {
                class: "public_interest_weather".into(),
                target_replicas: 3,
                replicated_replicas: 3,
                read_failover_ready: true,
                sla_met: true,
            },
            AvailabilitySlaStatus {
                class: "public_interest_climate".into(),
                target_replicas: 2,
                replicated_replicas: 1,
                read_failover_ready: false,
                sla_met: false,
            },
        ];

        let mut analytics = StorageAnalytics::new(10);
        let input = AnalyticsInput {
            providers: vec![],
            budget: &budget,
            tiering_config: &config,
            domain_index: &index,
            sla_statuses: &statuses,
            failover: &failover,
            tier_bytes: None,
            domain_bytes: None,
            timestamp: 1_000_000,
        };
        let snap = analytics.snapshot(&input);

        assert_eq!(snap.sla_compliance.len(), 2);

        let weather_sla = &snap.sla_compliance[0];
        assert_eq!(weather_sla.class, "public_interest_weather");
        assert_eq!(weather_sla.target_replicas, 3);
        assert_eq!(weather_sla.actual_replicas, 3);
        assert!(weather_sla.read_failover_ready);
        assert!(weather_sla.sla_met);

        let climate_sla = &snap.sla_compliance[1];
        assert!(!climate_sla.sla_met);
        assert!(!climate_sla.read_failover_ready);
    }

    // ---- Failover snapshot ----

    #[test]
    fn test_failover_snapshot_computed_rates() {
        let failover = FailoverMetrics {
            total_requests: 100,
            primary_successes: 80,
            failover_count: 15,
            total_failures: 5,
            integrity_checks: 95,
            integrity_failures: 2,
            store_latency_sum_ms: 1000,
            store_count: 10,
        };

        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));

        let f = &snap.failover;
        assert_eq!(f.total_requests, 100);
        assert_eq!(f.primary_successes, 80);
        assert_eq!(f.failover_count, 15);
        assert_eq!(f.total_failures, 5);
        assert!((f.avg_store_latency_ms - 100.0).abs() < f64::EPSILON);
        assert!((f.primary_success_rate - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_failover_snapshot_zero_requests() {
        let failover = FailoverMetrics::default();
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));

        assert_eq!(snap.failover.avg_store_latency_ms, 0.0);
        assert_eq!(snap.failover.primary_success_rate, 0.0);
    }

    // ---- Ring-buffer history ----

    #[test]
    fn test_history_accumulates_snapshots() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(3);

        for ts in [1u64, 2, 3] {
            let mut input = empty_input(&budget, &config, &index, &failover);
            input.timestamp = ts;
            analytics.snapshot(&input);
        }
        assert_eq!(analytics.history().len(), 3);
    }

    #[test]
    fn test_history_evicts_oldest_when_full() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(2);

        for ts in [10u64, 20, 30] {
            let mut input = empty_input(&budget, &config, &index, &failover);
            input.timestamp = ts;
            analytics.snapshot(&input);
        }
        // Oldest (ts=10) must be gone; ring holds [20, 30]
        let timestamps: Vec<u64> = analytics.history().iter().map(|s| s.timestamp).collect();
        assert_eq!(timestamps, vec![20, 30]);
    }

    #[test]
    fn test_history_disabled_when_limit_zero() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(0);

        for ts in [1u64, 2, 3] {
            let mut input = empty_input(&budget, &config, &index, &failover);
            input.timestamp = ts;
            analytics.snapshot(&input);
        }
        assert_eq!(analytics.history().len(), 0);
    }

    // ---- JSON export ----

    #[test]
    fn test_export_json_is_valid_json() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));

        let json = StorageAnalytics::export_json(&snap);
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("export_json must produce valid JSON");
        assert!(parsed.get("timestamp").is_some());
        assert!(parsed.get("providers").is_some());
        assert!(parsed.get("tiers").is_some());
        assert!(parsed.get("domains").is_some());
        assert!(parsed.get("budget").is_some());
        assert!(parsed.get("sla_compliance").is_some());
        assert!(parsed.get("failover").is_some());
    }

    #[test]
    fn test_export_json_contains_all_tiers() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));

        let json = StorageAnalytics::export_json(&snap);
        for tier in ["Hot", "Warm", "Cold", "Archive"] {
            assert!(json.contains(tier), "JSON missing tier: {tier}");
        }
    }

    // ---- Prometheus export ----

    #[test]
    fn test_export_prometheus_contains_help_and_type() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));

        let prom = StorageAnalytics::export_prometheus(&snap);
        assert!(prom.contains("# HELP"), "Prometheus output missing # HELP");
        assert!(prom.contains("# TYPE"), "Prometheus output missing # TYPE");
    }

    #[test]
    fn test_export_prometheus_provider_metrics() {
        let stats = StorageStats { total_bytes: 512, ..Default::default() };
        let metrics = InMemoryMetrics::new();
        metrics.record_archive_attempt("ipfs", true);

        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let input = AnalyticsInput {
            providers: vec![ProviderInput {
                name: "ipfs",
                healthy: true,
                stats: &stats,
                metrics: &metrics,
                cost_to_date: 1.23,
            }],
            budget: &budget,
            tiering_config: &config,
            domain_index: &index,
            sla_statuses: &[],
            failover: &failover,
            tier_bytes: None,
            domain_bytes: None,
            timestamp: 2_000_000,
        };
        let snap = analytics.snapshot(&input);
        let prom = StorageAnalytics::export_prometheus(&snap);

        assert!(
            prom.contains(r#"wrfcoin_storage_provider_objects_stored{provider="ipfs"}"#),
            "missing provider objects_stored metric"
        );
        assert!(
            prom.contains(r#"wrfcoin_storage_provider_healthy{provider="ipfs"} 1"#),
            "missing provider healthy=1 metric"
        );
    }

    #[test]
    fn test_export_prometheus_sla_metrics() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let statuses = vec![AvailabilitySlaStatus {
            class: "public_interest".into(),
            target_replicas: 3,
            replicated_replicas: 2,
            read_failover_ready: false,
            sla_met: false,
        }];
        let mut analytics = StorageAnalytics::new(10);
        let input = AnalyticsInput {
            providers: vec![],
            budget: &budget,
            tiering_config: &config,
            domain_index: &index,
            sla_statuses: &statuses,
            failover: &failover,
            tier_bytes: None,
            domain_bytes: None,
            timestamp: 3_000_000,
        };
        let snap = analytics.snapshot(&input);
        let prom = StorageAnalytics::export_prometheus(&snap);

        assert!(
            prom.contains(r#"wrfcoin_storage_sla_met{class="public_interest"} 0"#),
            "missing SLA met=0 metric"
        );
        assert!(
            prom.contains(r#"wrfcoin_storage_sla_actual_replicas{class="public_interest"} 2"#),
            "missing actual_replicas metric"
        );
    }

    #[test]
    fn test_export_prometheus_infinite_budget_uses_plus_inf() {
        let budget = StorageBudget::new(); // no limits → remaining = +Inf
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));
        let prom = StorageAnalytics::export_prometheus(&snap);
        assert!(
            prom.contains("+Inf"),
            "Prometheus output must use +Inf for unlimited budget"
        );
    }

    #[test]
    fn test_export_prometheus_domain_metrics_present() {
        let budget = StorageBudget::new();
        let config = TieringConfig::default();
        let index = DomainContentIndex::new();
        let failover = FailoverMetrics::default();
        let mut analytics = StorageAnalytics::new(10);
        let snap = analytics.snapshot(&empty_input(&budget, &config, &index, &failover));
        let prom = StorageAnalytics::export_prometheus(&snap);

        for domain in ["Weather", "AirQuality", "Soil", "Ocean", "Hydrology", "Wildfire", "WaterQuality"] {
            assert!(
                prom.contains(&format!(r#"domain="{domain}""#)),
                "Prometheus output missing domain: {domain}"
            );
        }
    }
}
