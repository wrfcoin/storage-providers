use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use prometheus::{IntCounter, Opts};
use serde::{Deserialize, Serialize};
use tracing::info;

use wrfcoin_filecoin::deals::{DealManager, DealStatus};
use wrfcoin_storage_common::{
    types::{DataTier, EnvironmentalDomain, QuarantineRecord},
    ContentId, DomainContentIndex, IndexEntry, StorageProvider,
};

use crate::retention::RetentionManager;

const DEFAULT_MARK_GRACE_SECS: u64 = 24 * 60 * 60;
const DEFAULT_QUARANTINE_RETENTION_SECS: u64 = 7 * 24 * 60 * 60;
const DEFAULT_TIER_PROMOTION_RETENTION_SECS: u64 = 24 * 60 * 60;
const ALL_DOMAINS: [EnvironmentalDomain; 7] = [
    EnvironmentalDomain::Weather,
    EnvironmentalDomain::AirQuality,
    EnvironmentalDomain::WaterQuality,
    EnvironmentalDomain::Soil,
    EnvironmentalDomain::Ocean,
    EnvironmentalDomain::Hydrology,
    EnvironmentalDomain::Wildfire,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GcKind {
    Content,
    Quarantine,
    FilecoinDeal,
    TierPromotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GcReason {
    Expired,
    Orphaned,
    ReinstatedQuarantine,
    ExpiredFilecoinDeal,
    SlashedFilecoinDeal,
    StaleTierPromotion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcMarkedItem {
    pub subject: String,
    pub kind: GcKind,
    pub reason: GcReason,
    pub marked_at: u64,
    pub eligible_after: u64,
    pub bytes_estimate: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcDeletionRecord {
    pub subject: String,
    pub kind: GcKind,
    pub reason: GcReason,
    pub bytes_freed: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcReport {
    pub content_marked: Vec<GcMarkedItem>,
    pub content_swept: Vec<GcDeletionRecord>,
    pub quarantine_removed: Vec<String>,
    pub filecoin_deals_removed: Vec<String>,
    pub tier_promotions_removed: Vec<String>,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
    pub interrupted: bool,
}

impl GcReport {
    pub fn is_empty(&self) -> bool {
        self.content_marked.is_empty()
            && self.content_swept.is_empty()
            && self.quarantine_removed.is_empty()
            && self.filecoin_deals_removed.is_empty()
            && self.tier_promotions_removed.is_empty()
            && self.bytes_freed == 0
            && self.errors.is_empty()
            && !self.interrupted
    }

    fn merge(&mut self, other: GcReport) {
        self.content_marked.extend(other.content_marked);
        self.content_swept.extend(other.content_swept);
        self.quarantine_removed.extend(other.quarantine_removed);
        self.filecoin_deals_removed
            .extend(other.filecoin_deals_removed);
        self.tier_promotions_removed
            .extend(other.tier_promotions_removed);
        self.bytes_freed += other.bytes_freed;
        self.errors.extend(other.errors);
        self.interrupted |= other.interrupted;
    }

    fn push_error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }
}

pub type GcSummary = GcReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcConfig {
    pub mark_grace_period_secs: u64,
    pub quarantine_retention_secs: u64,
    pub tier_promotion_retention_secs: u64,
    pub default_tier: DataTier,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            mark_grace_period_secs: DEFAULT_MARK_GRACE_SECS,
            quarantine_retention_secs: DEFAULT_QUARANTINE_RETENTION_SECS,
            tier_promotion_retention_secs: DEFAULT_TIER_PROMOTION_RETENTION_SECS,
            default_tier: DataTier::Cold,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GcConfigError {
    #[error("failed to read GC config from {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse GC config: {0}")]
    Parse(String),
}

impl GcConfig {
    pub fn from_toml_str(input: &str) -> Result<Self, GcConfigError> {
        toml::from_str(input).map_err(|error| GcConfigError::Parse(error.to_string()))
    }

    pub fn from_toml_file(path: impl AsRef<Path>) -> Result<Self, GcConfigError> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path).map_err(|source| GcConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&contents)
    }

    pub fn from_env() -> Result<Self, GcConfigError> {
        let mut config = Self::default();
        config.apply_env_overrides()?;
        Ok(config)
    }

    fn apply_env_overrides(&mut self) -> Result<(), GcConfigError> {
        if let Ok(value) = env::var("WRFCOIN_GC_MARK_GRACE_PERIOD_SECS") {
            self.mark_grace_period_secs = value.parse().map_err(|error| {
                GcConfigError::Parse(format!("WRFCOIN_GC_MARK_GRACE_PERIOD_SECS: {error}"))
            })?;
        }
        if let Ok(value) = env::var("WRFCOIN_GC_QUARANTINE_RETENTION_SECS") {
            self.quarantine_retention_secs = value.parse().map_err(|error| {
                GcConfigError::Parse(format!("WRFCOIN_GC_QUARANTINE_RETENTION_SECS: {error}"))
            })?;
        }
        if let Ok(value) = env::var("WRFCOIN_GC_TIER_PROMOTION_RETENTION_SECS") {
            self.tier_promotion_retention_secs = value.parse().map_err(|error| {
                GcConfigError::Parse(format!("WRFCOIN_GC_TIER_PROMOTION_RETENTION_SECS: {error}"))
            })?;
        }
        if let Ok(value) = env::var("WRFCOIN_GC_DEFAULT_TIER") {
            self.default_tier = parse_tier(&value)?;
        }

        Ok(())
    }

    pub fn load() -> Result<Self, GcConfigError> {
        let mut config = if let Ok(path) = env::var("WRFCOIN_GC_CONFIG") {
            Self::from_toml_file(path)
        } else if Path::new("gc.toml").exists() {
            Self::from_toml_file("gc.toml")
        } else {
            Ok(Self::default())
        }?;

        config.apply_env_overrides()?;
        Ok(config)
    }
}

#[derive(Debug)]
pub struct GcMetrics {
    pub items_marked: IntCounter,
    pub items_swept: IntCounter,
    pub bytes_freed: IntCounter,
    pub errors: IntCounter,
}

impl GcMetrics {
    pub fn new() -> Self {
        Self {
            items_marked: IntCounter::with_opts(Opts::new(
                "gc_items_marked_total",
                "Total GC items marked for deletion",
            ))
            .expect("gc_items_marked_total counter"),
            items_swept: IntCounter::with_opts(Opts::new(
                "gc_items_swept_total",
                "Total GC items swept from storage",
            ))
            .expect("gc_items_swept_total counter"),
            bytes_freed: IntCounter::with_opts(Opts::new(
                "gc_bytes_freed_total",
                "Total bytes reclaimed by GC",
            ))
            .expect("gc_bytes_freed_total counter"),
            errors: IntCounter::with_opts(Opts::new("gc_errors_total", "Total GC errors"))
                .expect("gc_errors_total counter"),
        }
    }
}

impl Default for GcMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
struct PendingDeletion {
    subject: String,
    kind: GcKind,
    reason: GcReason,
    marked_at: u64,
    eligible_after: u64,
    bytes_estimate: u64,
    content_id: Option<ContentId>,
    reference_count_at_mark: Option<usize>,
}

impl PendingDeletion {
    fn marked_item(&self) -> GcMarkedItem {
        GcMarkedItem {
            subject: self.subject.clone(),
            kind: self.kind,
            reason: self.reason,
            marked_at: self.marked_at,
            eligible_after: self.eligible_after,
            bytes_estimate: self.bytes_estimate,
        }
    }

    fn deletion_record(&self) -> GcDeletionRecord {
        GcDeletionRecord {
            subject: self.subject.clone(),
            kind: self.kind,
            reason: self.reason,
            bytes_freed: self.bytes_estimate,
        }
    }
}

fn parse_tier(value: &str) -> Result<DataTier, GcConfigError> {
    match value.to_ascii_lowercase().as_str() {
        "hot" => Ok(DataTier::Hot),
        "warm" => Ok(DataTier::Warm),
        "cold" => Ok(DataTier::Cold),
        "archive" => Ok(DataTier::Archive),
        other => Err(GcConfigError::Parse(format!("invalid tier value: {other}"))),
    }
}

fn current_time_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn content_key(content_id: &ContentId) -> String {
    content_id.to_string()
}

fn quarantine_key(quarantine_id: &str) -> String {
    format!("quarantine:{quarantine_id}")
}

fn deal_key(deal_id: &str) -> String {
    format!("deal:{deal_id}")
}

fn promotion_key(content_id: &str) -> String {
    format!("promotion:{content_id}")
}

fn artifact_class_for_domain(domain: &EnvironmentalDomain) -> &'static str {
    match domain {
        EnvironmentalDomain::Weather => "weather.observation",
        EnvironmentalDomain::AirQuality => "air_quality.observation",
        EnvironmentalDomain::WaterQuality => "water_quality.observation",
        EnvironmentalDomain::Soil => "soil.observation",
        EnvironmentalDomain::Ocean => "ocean.observation",
        EnvironmentalDomain::Hydrology => "hydrology.observation",
        EnvironmentalDomain::Wildfire => "wildfire.observation",
        _ => "weather.observation",
    }
}

fn content_size_estimate(content_sizes: &HashMap<String, u64>, subject: &str) -> u64 {
    content_sizes.get(subject).copied().unwrap_or(0)
}

/// Garbage collector for expired and orphaned content.
pub struct GarbageCollector {
    retention_manager: RetentionManager,
    config: GcConfig,
    content_index: DomainContentIndex,
    quarantine_records: HashMap<String, QuarantineRecord>,
    filecoin_deal_manager: Option<Arc<RwLock<DealManager>>>,
    tier_promotions: HashMap<String, u64>,
    pending_deletions: HashMap<String, PendingDeletion>,
    reference_counts: HashMap<String, usize>,
    content_sizes: HashMap<String, u64>,
    interrupt_after_deletes: Option<usize>,
    metrics: GcMetrics,
}

impl GarbageCollector {
    pub fn new(retention_manager: RetentionManager) -> Self {
        Self::with_config(retention_manager, GcConfig::default())
    }

    pub fn with_config(retention_manager: RetentionManager, config: GcConfig) -> Self {
        Self {
            retention_manager,
            config,
            content_index: DomainContentIndex::new(),
            quarantine_records: HashMap::new(),
            filecoin_deal_manager: None,
            tier_promotions: HashMap::new(),
            pending_deletions: HashMap::new(),
            reference_counts: HashMap::new(),
            content_sizes: HashMap::new(),
            interrupt_after_deletes: None,
            metrics: GcMetrics::new(),
        }
    }

    pub fn config(&self) -> &GcConfig {
        &self.config
    }

    pub fn metrics(&self) -> &GcMetrics {
        &self.metrics
    }

    pub fn set_deal_manager(&mut self, deal_manager: Arc<RwLock<DealManager>>) {
        self.filecoin_deal_manager = Some(deal_manager);
    }

    pub fn set_interrupt_after_deletes(&mut self, limit: Option<usize>) {
        self.interrupt_after_deletes = limit;
    }

    pub fn add_content(&mut self, entry: IndexEntry) {
        let key = content_key(&entry.content_id);
        self.reference_counts.entry(key.clone()).or_insert(1);
        self.content_index.insert(entry);
    }

    pub fn record_content_size(&mut self, content_id: impl Into<String>, size_bytes: u64) {
        self.content_sizes.insert(content_id.into(), size_bytes);
    }

    pub fn set_reference_count(&mut self, content_id: impl Into<String>, count: usize) {
        self.reference_counts.insert(content_id.into(), count);
    }

    pub fn bump_reference(&mut self, content_id: impl Into<String>) {
        let key = content_id.into();
        *self.reference_counts.entry(key).or_insert(0) += 1;
    }

    pub fn release_reference(&mut self, content_id: impl Into<String>) {
        let key = content_id.into();
        let entry = self.reference_counts.entry(key).or_insert(0);
        *entry = entry.saturating_sub(1);
    }

    pub fn add_quarantine(&mut self, content_id: String, record: QuarantineRecord) {
        self.quarantine_records.insert(content_id, record);
    }

    pub fn add_tier_promotion(&mut self, content_id: String, promoted_at_secs: u64) {
        self.tier_promotions.insert(content_id, promoted_at_secs);
    }

    pub fn pending_count(&self) -> usize {
        self.pending_deletions.len()
    }

    pub fn pending_subjects(&self) -> Vec<String> {
        let mut subjects: Vec<_> = self.pending_deletions.keys().cloned().collect();
        subjects.sort();
        subjects
    }

    pub async fn mark_phase_at(
        &mut self,
        current_time_secs: u64,
        tier: DataTier,
        provider: &dyn StorageProvider,
    ) -> GcReport {
        let mut report = GcReport::default();

        for domain in ALL_DOMAINS {
            let entries = self.content_index.query_by_domain(&domain).to_vec();
            for entry in entries {
                let subject = content_key(&entry.content_id);
                if self.pending_deletions.contains_key(&subject) {
                    continue;
                }

                let artifact_class = artifact_class_for_domain(&domain);
                let age_secs = current_time_secs.saturating_sub(entry.stored_at);
                let expired =
                    !self
                        .retention_manager
                        .should_retain(artifact_class, &tier, age_secs, 0.5);
                let exists = match provider.exists(&entry.content_id).await {
                    Ok(value) => value,
                    Err(error) => {
                        let msg = format!("exists check failed for {subject}: {error}");
                        report.push_error(msg);
                        self.metrics.errors.inc();
                        continue;
                    }
                };
                let ref_count = self.reference_counts.get(&subject).copied().unwrap_or(0);
                let orphaned = !exists || ref_count == 0;

                if expired || orphaned {
                    let reason = if orphaned {
                        GcReason::Orphaned
                    } else {
                        GcReason::Expired
                    };
                    let pending = PendingDeletion {
                        subject: subject.clone(),
                        kind: GcKind::Content,
                        reason,
                        marked_at: current_time_secs,
                        eligible_after: current_time_secs + self.config.mark_grace_period_secs,
                        bytes_estimate: content_size_estimate(&self.content_sizes, &subject),
                        content_id: Some(entry.content_id.clone()),
                        reference_count_at_mark: Some(ref_count),
                    };
                    self.metrics.items_marked.inc();
                    report.content_marked.push(pending.marked_item());
                    self.pending_deletions.insert(subject, pending);
                }
            }
        }

        for (quarantine_id, record) in self.quarantine_records.clone() {
            if !record.reinstated {
                continue;
            }
            let age_secs = current_time_secs.saturating_sub(record.detected_at);
            if age_secs < self.config.quarantine_retention_secs {
                continue;
            }

            let key = quarantine_key(&quarantine_id);
            if self.pending_deletions.contains_key(&key) {
                continue;
            }

            let pending = PendingDeletion {
                subject: key.clone(),
                kind: GcKind::Quarantine,
                reason: GcReason::ReinstatedQuarantine,
                marked_at: current_time_secs,
                eligible_after: current_time_secs + self.config.mark_grace_period_secs,
                bytes_estimate: 0,
                content_id: None,
                reference_count_at_mark: None,
            };
            self.metrics.items_marked.inc();
            report.content_marked.push(pending.marked_item());
            self.pending_deletions.insert(key, pending);
        }

        if let Some(manager) = &self.filecoin_deal_manager {
            match manager.read() {
                Ok(guard) => {
                    for deal in guard.iter() {
                        let reason = match deal.status {
                            DealStatus::Expired => Some(GcReason::ExpiredFilecoinDeal),
                            DealStatus::Slashed => Some(GcReason::SlashedFilecoinDeal),
                            _ => None,
                        };
                        let Some(reason) = reason else {
                            continue;
                        };

                        let subject = deal_key(&deal.deal_id);
                        if self.pending_deletions.contains_key(&subject) {
                            continue;
                        }

                        let pending = PendingDeletion {
                            subject: subject.clone(),
                            kind: GcKind::FilecoinDeal,
                            reason,
                            marked_at: current_time_secs,
                            eligible_after: current_time_secs + self.config.mark_grace_period_secs,
                            bytes_estimate: deal.size,
                            content_id: None,
                            reference_count_at_mark: None,
                        };
                        self.metrics.items_marked.inc();
                        report.content_marked.push(pending.marked_item());
                        self.pending_deletions.insert(subject, pending);
                    }
                }
                Err(error) => {
                    let msg = format!("failed to read filecoin deal manager: {error}");
                    report.push_error(msg);
                    self.metrics.errors.inc();
                }
            }
        }

        for (content_id, promoted_at_secs) in self.tier_promotions.clone() {
            let age_secs = current_time_secs.saturating_sub(promoted_at_secs);
            if age_secs < self.config.tier_promotion_retention_secs {
                continue;
            }

            let key = promotion_key(&content_id);
            if self.pending_deletions.contains_key(&key) {
                continue;
            }

            let pending = PendingDeletion {
                subject: key.clone(),
                kind: GcKind::TierPromotion,
                reason: GcReason::StaleTierPromotion,
                marked_at: current_time_secs,
                eligible_after: current_time_secs + self.config.mark_grace_period_secs,
                bytes_estimate: 0,
                content_id: None,
                reference_count_at_mark: None,
            };
            self.metrics.items_marked.inc();
            report.content_marked.push(pending.marked_item());
            self.pending_deletions.insert(key, pending);
        }

        report
    }

    pub async fn sweep(&mut self, dry_run: bool, provider: &dyn StorageProvider) -> GcReport {
        self.sweep_at(current_time_secs(), dry_run, provider).await
    }

    pub async fn sweep_at(
        &mut self,
        current_time_secs: u64,
        dry_run: bool,
        provider: &dyn StorageProvider,
    ) -> GcReport {
        self.sweep_internal(
            current_time_secs,
            self.config.default_tier,
            self.config.tier_promotion_retention_secs,
            dry_run,
            provider,
        )
        .await
    }

    pub async fn gc_sweep(
        &mut self,
        current_time_secs: u64,
        tier_promotion_max_age_secs: u64,
        tier: DataTier,
        dry_run: bool,
        provider: &dyn StorageProvider,
    ) -> GcReport {
        self.sweep_internal(
            current_time_secs,
            tier,
            tier_promotion_max_age_secs,
            dry_run,
            provider,
        )
        .await
    }

    pub async fn gc_sweep_now(&mut self, provider: &dyn StorageProvider) -> GcReport {
        self.sweep(false, provider).await
    }

    async fn sweep_internal(
        &mut self,
        current_time_secs: u64,
        tier: DataTier,
        tier_promotion_max_age_secs: u64,
        dry_run: bool,
        provider: &dyn StorageProvider,
    ) -> GcReport {
        let original_tier = self.config.default_tier;
        let original_promotion = self.config.tier_promotion_retention_secs;
        self.config.default_tier = tier;
        self.config.tier_promotion_retention_secs = tier_promotion_max_age_secs;

        let mut report = if dry_run {
            self.preview_at(current_time_secs, tier, provider).await
        } else {
            self.mark_phase_at(current_time_secs, tier, provider).await
        };

        if !dry_run {
            let sweep_report = self.sweep_pending_at(current_time_secs, provider).await;
            report.merge(sweep_report);
        }

        self.config.default_tier = original_tier;
        self.config.tier_promotion_retention_secs = original_promotion;

        report
    }

    async fn preview_at(
        &self,
        current_time_secs: u64,
        tier: DataTier,
        provider: &dyn StorageProvider,
    ) -> GcReport {
        let mut report = GcReport::default();

        for domain in ALL_DOMAINS {
            for entry in self.content_index.query_by_domain(&domain) {
                let subject = content_key(&entry.content_id);
                if self.pending_deletions.contains_key(&subject) {
                    continue;
                }
                let artifact_class = artifact_class_for_domain(&domain);
                let age_secs = current_time_secs.saturating_sub(entry.stored_at);
                let expired =
                    !self
                        .retention_manager
                        .should_retain(artifact_class, &tier, age_secs, 0.5);
                let exists = match provider.exists(&entry.content_id).await {
                    Ok(value) => value,
                    Err(error) => {
                        report.push_error(format!("exists check failed for {subject}: {error}"));
                        continue;
                    }
                };
                let ref_count = self.reference_counts.get(&subject).copied().unwrap_or(0);
                let orphaned = !exists || ref_count == 0;
                if expired || orphaned {
                    let reason = if orphaned {
                        GcReason::Orphaned
                    } else {
                        GcReason::Expired
                    };
                    report.content_marked.push(GcMarkedItem {
                        subject,
                        kind: GcKind::Content,
                        reason,
                        marked_at: current_time_secs,
                        eligible_after: current_time_secs + self.config.mark_grace_period_secs,
                        bytes_estimate: content_size_estimate(
                            &self.content_sizes,
                            &content_key(&entry.content_id),
                        ),
                    });
                }
            }
        }

        for (quarantine_id, record) in &self.quarantine_records {
            if !record.reinstated {
                continue;
            }
            if current_time_secs.saturating_sub(record.detected_at)
                < self.config.quarantine_retention_secs
            {
                continue;
            }
            let key = quarantine_key(quarantine_id);
            if self.pending_deletions.contains_key(&key) {
                continue;
            }
            report.content_marked.push(GcMarkedItem {
                subject: key,
                kind: GcKind::Quarantine,
                reason: GcReason::ReinstatedQuarantine,
                marked_at: current_time_secs,
                eligible_after: current_time_secs + self.config.mark_grace_period_secs,
                bytes_estimate: 0,
            });
        }

        if let Some(manager) = &self.filecoin_deal_manager {
            match manager.read() {
                Ok(guard) => {
                    for deal in guard.iter() {
                        let reason = match deal.status {
                            DealStatus::Expired => Some(GcReason::ExpiredFilecoinDeal),
                            DealStatus::Slashed => Some(GcReason::SlashedFilecoinDeal),
                            _ => None,
                        };
                        let Some(reason) = reason else {
                            continue;
                        };
                        let key = deal_key(&deal.deal_id);
                        if self.pending_deletions.contains_key(&key) {
                            continue;
                        }
                        report.content_marked.push(GcMarkedItem {
                            subject: key,
                            kind: GcKind::FilecoinDeal,
                            reason,
                            marked_at: current_time_secs,
                            eligible_after: current_time_secs + self.config.mark_grace_period_secs,
                            bytes_estimate: deal.size,
                        });
                    }
                }
                Err(error) => {
                    report.push_error(format!("failed to read filecoin deal manager: {error}"))
                }
            }
        }

        for (content_id, promoted_at_secs) in &self.tier_promotions {
            if current_time_secs.saturating_sub(*promoted_at_secs)
                < self.config.tier_promotion_retention_secs
            {
                continue;
            }
            let key = promotion_key(content_id);
            if self.pending_deletions.contains_key(&key) {
                continue;
            }
            report.content_marked.push(GcMarkedItem {
                subject: key,
                kind: GcKind::TierPromotion,
                reason: GcReason::StaleTierPromotion,
                marked_at: current_time_secs,
                eligible_after: current_time_secs + self.config.mark_grace_period_secs,
                bytes_estimate: 0,
            });
        }

        for pending in self.pending_deletions.values() {
            if pending.eligible_after <= current_time_secs {
                report.content_swept.push(pending.deletion_record());
            }
        }

        report
    }

    async fn sweep_pending_at(
        &mut self,
        current_time_secs: u64,
        provider: &dyn StorageProvider,
    ) -> GcReport {
        let mut report = GcReport::default();
        let mut subjects: Vec<_> = self.pending_deletions.keys().cloned().collect();
        subjects.sort();
        let mut deleted_this_run = 0usize;

        for subject in subjects {
            let Some(pending) = self.pending_deletions.get(&subject).cloned() else {
                continue;
            };
            if pending.eligible_after > current_time_secs {
                continue;
            }

            match pending.kind {
                GcKind::Content => {
                    let Some(content_id) = pending.content_id.clone() else {
                        self.pending_deletions.remove(&subject);
                        continue;
                    };
                    let ref_count = self.reference_counts.get(&subject).copied().unwrap_or(0);
                    if let Some(marked_ref_count) = pending.reference_count_at_mark {
                        if ref_count > marked_ref_count {
                            self.pending_deletions.remove(&subject);
                            continue;
                        }
                    }
                    if pending.reference_count_at_mark.is_none() && ref_count > 0 {
                        self.pending_deletions.remove(&subject);
                        continue;
                    }

                    let exists = match provider.exists(&content_id).await {
                        Ok(value) => value,
                        Err(error) => {
                            report
                                .push_error(format!("exists check failed for {subject}: {error}"));
                            self.metrics.errors.inc();
                            continue;
                        }
                    };

                    if exists {
                        if let Err(error) = provider.delete(&content_id).await {
                            report.push_error(format!("delete failed for {subject}: {error}"));
                            self.metrics.errors.inc();
                            continue;
                        }
                    }

                    self.remove_content_entry(&content_id);
                    self.reference_counts.remove(&subject);
                    self.content_sizes.remove(&subject);
                    self.pending_deletions.remove(&subject);
                    self.metrics.items_swept.inc();
                    self.metrics.bytes_freed.inc_by(pending.bytes_estimate);
                    report.bytes_freed += pending.bytes_estimate;
                    report.content_swept.push(pending.deletion_record());
                    info!(
                        cid = %subject,
                        reason = ?pending.reason,
                        bytes_freed = pending.bytes_estimate,
                        "gc swept content"
                    );
                }
                GcKind::Quarantine => {
                    self.quarantine_records
                        .remove(&subject["quarantine:".len()..]);
                    self.pending_deletions.remove(&subject);
                    self.metrics.items_swept.inc();
                    report.quarantine_removed.push(subject);
                    report.content_swept.push(pending.deletion_record());
                }
                GcKind::FilecoinDeal => {
                    if let Some(manager) = &self.filecoin_deal_manager {
                        match manager.write() {
                            Ok(mut guard) => {
                                let deal_id = subject["deal:".len()..].to_string();
                                let removed = guard.remove_deal(&deal_id);
                                self.pending_deletions.remove(&subject);
                                if let Some(deal) = removed {
                                    self.metrics.items_swept.inc();
                                    self.metrics.bytes_freed.inc_by(deal.size);
                                    report.bytes_freed += deal.size;
                                    report.filecoin_deals_removed.push(deal.deal_id.clone());
                                    report.content_swept.push(pending.deletion_record());
                                    info!(
                                        deal_id = %deal.deal_id,
                                        reason = ?pending.reason,
                                        bytes_freed = deal.size,
                                        "gc swept filecoin deal"
                                    );
                                } else {
                                    report.push_error(format!(
                                        "filecoin deal not found for {subject}"
                                    ));
                                }
                            }
                            Err(error) => {
                                report.push_error(format!(
                                    "failed to lock filecoin deal manager: {error}"
                                ));
                                self.metrics.errors.inc();
                            }
                        }
                    }
                }
                GcKind::TierPromotion => {
                    self.tier_promotions.remove(&subject["promotion:".len()..]);
                    self.pending_deletions.remove(&subject);
                    self.metrics.items_swept.inc();
                    report.tier_promotions_removed.push(subject);
                    report.content_swept.push(pending.deletion_record());
                }
            }

            deleted_this_run += 1;
            if matches!(self.interrupt_after_deletes, Some(limit) if deleted_this_run >= limit) {
                report.interrupted = true;
                break;
            }
        }

        report
    }

    fn remove_content_entry(&mut self, content_id: &ContentId) {
        let key = content_key(content_id);
        self.content_index.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use wrfcoin_storage_common::StorageError;

    #[derive(Default)]
    struct MockProvider {
        existing: Arc<Mutex<HashSet<String>>>,
        deleted: Arc<Mutex<Vec<String>>>,
        fail_exists: Arc<Mutex<HashSet<String>>>,
        fail_delete: Arc<Mutex<HashSet<String>>>,
    }

    impl MockProvider {
        fn with_existing(ids: &[&str]) -> Self {
            let provider = Self::default();
            provider
                .existing
                .lock()
                .unwrap()
                .extend(ids.iter().map(|id| (*id).to_string()));
            provider
        }

        fn deleted(&self) -> Vec<String> {
            self.deleted.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl StorageProvider for MockProvider {
        async fn store(
            &self,
            _data: &[u8],
            _metadata: &wrfcoin_storage_common::Metadata,
        ) -> Result<ContentId, StorageError> {
            Ok(ContentId {
                provider: "mock".into(),
                hash: "mock".into(),
            })
        }

        async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
            Ok(id.hash.as_bytes().to_vec())
        }

        async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
            let key = id.to_string();
            if self.fail_exists.lock().unwrap().contains(&key) {
                return Err(StorageError::ProviderError("exists failed".into()));
            }
            Ok(self.existing.lock().unwrap().contains(&key))
        }

        async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
            let key = id.to_string();
            if self.fail_delete.lock().unwrap().contains(&key) {
                return Err(StorageError::ProviderError("delete failed".into()));
            }
            self.existing.lock().unwrap().remove(&key);
            self.deleted.lock().unwrap().push(key);
            Ok(())
        }

        async fn health_check(&self) -> Result<wrfcoin_storage_common::HealthStatus, StorageError> {
            Ok(wrfcoin_storage_common::HealthStatus {
                healthy: true,
                provider: "mock".into(),
                latency_ms: None,
                message: None,
            })
        }

        async fn stats(&self) -> Result<wrfcoin_storage_common::StorageStats, StorageError> {
            Ok(wrfcoin_storage_common::StorageStats::default())
        }
    }

    fn content_entry(hash: &str, stored_at: u64, domain: EnvironmentalDomain) -> IndexEntry {
        IndexEntry {
            content_id: ContentId {
                provider: "mock".into(),
                hash: hash.into(),
            },
            domain,
            station_id: "station-1".into(),
            report_id: format!("report-{hash}"),
            stored_at,
        }
    }

    fn make_gc_with_grace(mark_grace_period_secs: u64) -> GarbageCollector {
        let mut retention = RetentionManager::new();
        let mut weather =
            wrfcoin_storage_common::RetentionPolicy::new("weather.observation", DataTier::Cold, 1);
        weather.max_age_secs = 1_000;
        retention.add_policy(weather);
        let mut config = GcConfig::default();
        config.mark_grace_period_secs = mark_grace_period_secs;
        config.quarantine_retention_secs = 200;
        config.tier_promotion_retention_secs = 150;
        GarbageCollector::with_config(retention, config)
    }

    fn make_gc() -> GarbageCollector {
        make_gc_with_grace(100)
    }

    fn attach_deal_manager(
        gc: &mut GarbageCollector,
        deals: DealManager,
    ) -> Arc<RwLock<DealManager>> {
        let manager = Arc::new(RwLock::new(deals));
        gc.set_deal_manager(manager.clone());
        manager
    }

    #[tokio::test]
    async fn mark_phase_identifies_expired_content() {
        let mut gc = make_gc();
        let provider = MockProvider::with_existing(&["mock:expired"]);
        let entry = content_entry("expired", 0, EnvironmentalDomain::Weather);
        gc.add_content(entry);
        gc.record_content_size("mock:expired", 512);

        let report = gc.mark_phase_at(2_000, DataTier::Cold, &provider).await;

        assert_eq!(report.content_marked.len(), 1);
        assert_eq!(report.content_marked[0].reason, GcReason::Expired);
        assert_eq!(gc.pending_count(), 1);
    }

    #[tokio::test]
    async fn mark_phase_identifies_orphaned_content() {
        let mut gc = make_gc();
        let provider = MockProvider::default();
        let entry = content_entry("orphaned", 0, EnvironmentalDomain::Weather);
        gc.add_content(entry);
        gc.set_reference_count("mock:orphaned", 0);

        let report = gc.mark_phase_at(100, DataTier::Cold, &provider).await;

        assert_eq!(report.content_marked.len(), 1);
        assert_eq!(report.content_marked[0].reason, GcReason::Orphaned);
        assert_eq!(gc.pending_count(), 1);
    }

    #[tokio::test]
    async fn sweep_respects_grace_period() {
        let mut gc = make_gc();
        let provider = MockProvider::with_existing(&["mock:expired"]);
        gc.add_content(content_entry("expired", 0, EnvironmentalDomain::Weather));

        let first = gc.sweep_at(2_000, false, &provider).await;
        assert_eq!(first.content_swept.len(), 0);
        assert_eq!(gc.pending_count(), 1);

        let second = gc.sweep_at(2_100, false, &provider).await;
        assert_eq!(second.content_swept.len(), 1);
        assert_eq!(provider.deleted(), vec!["mock:expired".to_string()]);
        assert_eq!(gc.pending_count(), 0);
    }

    #[tokio::test]
    async fn reference_race_survives_between_mark_and_sweep() {
        let mut gc = make_gc();
        let provider = MockProvider::with_existing(&["mock:race"]);
        gc.add_content(content_entry("race", 0, EnvironmentalDomain::Weather));
        gc.set_reference_count("mock:race", 0);

        let _ = gc.mark_phase_at(2_000, DataTier::Cold, &provider).await;
        gc.bump_reference("mock:race");

        let sweep = gc.sweep_at(2_100, false, &provider).await;

        assert!(sweep.content_swept.is_empty());
        assert!(provider.deleted().is_empty());
        assert_eq!(gc.pending_count(), 0);
    }

    #[tokio::test]
    async fn dry_run_has_no_side_effects() {
        let mut gc = make_gc();
        let provider = MockProvider::with_existing(&["mock:dry"]);
        gc.add_content(content_entry("dry", 0, EnvironmentalDomain::Weather));

        let report = gc.sweep_at(2_000, true, &provider).await;

        assert_eq!(report.content_marked.len(), 1);
        assert!(report.content_swept.is_empty());
        assert!(provider.deleted().is_empty());
        assert_eq!(gc.pending_count(), 0);
    }

    #[tokio::test]
    async fn quarantine_cleanup_waits_for_threshold() {
        let mut gc = make_gc_with_grace(0);
        let provider = MockProvider::default();
        gc.add_quarantine(
            "mock:quarantine".into(),
            QuarantineRecord {
                quarantine_id: "mock:quarantine".into(),
                reason: "reviewed".into(),
                detected_at: 0,
                review_required: false,
                reinstated: true,
            },
        );

        let too_soon = gc.sweep_at(150, false, &provider).await;
        assert!(too_soon.quarantine_removed.is_empty());

        let ready = gc.sweep_at(400, false, &provider).await;
        assert_eq!(
            ready.quarantine_removed,
            vec!["quarantine:mock:quarantine".to_string()]
        );
    }

    #[tokio::test]
    async fn expired_filecoin_deals_are_removed() {
        let mut gc = make_gc_with_grace(0);
        let provider = MockProvider::default();
        let mut deals = DealManager::new();
        let active = deals.propose_deal(
            "miner".into(),
            "client".into(),
            "cid-active".into(),
            10,
            1,
            10,
        );
        let expired = deals.propose_deal(
            "miner".into(),
            "client".into(),
            "cid-expired".into(),
            20,
            1,
            10,
        );
        deals.activate_deal(&active, 1).unwrap();
        deals.activate_deal(&expired, 1).unwrap();
        deals
            .set_deal_status(&expired, DealStatus::Expired)
            .unwrap();
        let manager = attach_deal_manager(&mut gc, deals);

        let report = gc.sweep_at(500, false, &provider).await;

        assert!(report
            .filecoin_deals_removed
            .contains(&"deal-2".to_string()));
        assert!(manager.read().unwrap().get_deal("deal-2").is_none());
    }

    #[tokio::test]
    async fn slashed_filecoin_deals_are_removed() {
        let mut gc = make_gc_with_grace(0);
        let provider = MockProvider::default();
        let mut deals = DealManager::new();
        let slashed = deals.propose_deal(
            "miner".into(),
            "client".into(),
            "cid-slashed".into(),
            20,
            1,
            10,
        );
        deals.activate_deal(&slashed, 1).unwrap();
        deals
            .set_deal_status(&slashed, DealStatus::Slashed)
            .unwrap();
        let manager = attach_deal_manager(&mut gc, deals);

        let report = gc.sweep_at(500, false, &provider).await;

        assert!(report
            .filecoin_deals_removed
            .contains(&"deal-1".to_string()));
        assert!(manager.read().unwrap().get_deal("deal-1").is_none());
    }

    #[tokio::test]
    async fn tier_promotion_cleanup_uses_threshold() {
        let mut gc = make_gc_with_grace(0);
        let provider = MockProvider::default();
        gc.add_tier_promotion("mock:promo".into(), 0);

        let report = gc.sweep_at(200, false, &provider).await;

        assert_eq!(
            report.tier_promotions_removed,
            vec!["promotion:mock:promo".to_string()]
        );
    }

    #[tokio::test]
    async fn repeated_sweeps_are_idempotent() {
        let mut gc = make_gc();
        let provider = MockProvider::with_existing(&["mock:idempotent"]);
        gc.add_content(content_entry("idempotent", 0, EnvironmentalDomain::Weather));

        let first = gc.sweep_at(2_000, false, &provider).await;
        let second = gc.sweep_at(2_100, false, &provider).await;

        assert_eq!(first.content_swept.len(), 0);
        assert_eq!(first.content_marked.len(), 1);
        assert_eq!(second.content_swept.len(), 1);
        assert_eq!(provider.deleted(), vec!["mock:idempotent".to_string()]);
        assert_eq!(gc.pending_count(), 0);
    }

    #[tokio::test]
    async fn crash_mid_sweep_can_resume_without_double_delete() {
        let mut gc = make_gc_with_grace(0);
        let provider = MockProvider::with_existing(&["mock:first", "mock:second"]);
        gc.add_content(content_entry("first", 0, EnvironmentalDomain::Weather));
        gc.add_content(content_entry("second", 0, EnvironmentalDomain::Weather));
        gc.set_interrupt_after_deletes(Some(1));

        let interrupted = gc.sweep_at(2_000, false, &provider).await;
        assert!(interrupted.interrupted);
        assert_eq!(provider.deleted().len(), 1);

        gc.set_interrupt_after_deletes(None);
        let resumed = gc.sweep_at(2_200, false, &provider).await;
        assert!(!resumed.interrupted);
        assert_eq!(provider.deleted().len(), 2);
        assert_eq!(gc.pending_count(), 0);
    }

    #[tokio::test]
    async fn gc_metrics_increment_for_marks_and_sweeps() {
        let mut gc = make_gc();
        let provider = MockProvider::with_existing(&["mock:metric"]);
        gc.record_content_size("mock:metric", 128);
        gc.add_content(content_entry("metric", 0, EnvironmentalDomain::Weather));

        let _ = gc.mark_phase_at(2_000, DataTier::Cold, &provider).await;
        let _ = gc.sweep_at(2_200, false, &provider).await;

        assert!(gc.metrics.items_marked.get() >= 1);
        assert!(gc.metrics.items_swept.get() >= 1);
        assert!(gc.metrics.bytes_freed.get() >= 128);
    }

    #[test]
    fn gc_config_can_load_from_toml() {
        let config = GcConfig::from_toml_str(
            r#"
mark_grace_period_secs = 42
quarantine_retention_secs = 77
tier_promotion_retention_secs = 88
default_tier = "Warm"
"#,
        )
        .unwrap();

        assert_eq!(config.mark_grace_period_secs, 42);
        assert_eq!(config.quarantine_retention_secs, 77);
        assert_eq!(config.tier_promotion_retention_secs, 88);
        assert_eq!(config.default_tier, DataTier::Warm);
    }

    #[test]
    fn gc_config_defaults_are_reasonable() {
        let config = GcConfig::default();
        assert_eq!(config.mark_grace_period_secs, 86_400);
        assert_eq!(config.quarantine_retention_secs, 604_800);
        assert_eq!(config.tier_promotion_retention_secs, 86_400);
        assert_eq!(config.default_tier, DataTier::Cold);
    }
}
