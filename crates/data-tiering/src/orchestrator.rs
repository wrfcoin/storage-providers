use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use wrfcoin_storage_common::{
    AdmissionDecision, AvailabilitySlaStatus, ContentId, DataTier, EnvironmentalReport, Metadata,
    ProvenanceChain, ProvenanceValidationResult, PublicInterestAvailabilityPolicy, QuarantineRecord,
    StorageError, StorageProvider, StorageStats,
};
use wrfcoin_storage_common::provenance::{
    ProvenancePolicy, ProvenanceValidator,
};

use crate::tiers::TieringConfig;

/// Orchestrates storage across multiple tiers (hot/warm/cold/archive).
///
/// Routes store and retrieve requests to the appropriate provider based on
/// the configured tiering policy.
pub struct DataTieringOrchestrator {
    tiers: HashMap<DataTier, Box<dyn StorageProvider>>,
    tier_config: TieringConfig,
    quarantine_records: Arc<RwLock<HashMap<String, QuarantineRecord>>>,
    public_interest_policies: Arc<RwLock<HashMap<String, PublicInterestAvailabilityPolicy>>>,
    provenance_policy: Arc<RwLock<ProvenancePolicy>>,
}

#[derive(Debug, Clone)]
pub struct PublicInterestStoreResult {
    pub class: String,
    pub replicated_content_ids: Vec<ContentId>,
    pub sla: AvailabilitySlaStatus,
}

impl DataTieringOrchestrator {
    /// Create a new orchestrator with the given tiering configuration.
    ///
    /// Providers are registered per-tier after construction via `register_tier`.
    pub fn new(tier_config: TieringConfig) -> Self {
        Self {
            tiers: HashMap::new(),
            tier_config,
            quarantine_records: Arc::new(RwLock::new(HashMap::new())),
            public_interest_policies: Arc::new(RwLock::new(HashMap::new())),
            provenance_policy: Arc::new(RwLock::new(ProvenancePolicy::default())),
        }
    }

    /// Register a storage provider for a specific tier.
    pub fn register_tier(&mut self, tier: DataTier, provider: Box<dyn StorageProvider>) {
        self.tiers.insert(tier, provider);
    }

    /// Store data in the specified tier.
    pub async fn store_with_tier(
        &self,
        tier: &DataTier,
        data: &[u8],
        metadata: &Metadata,
    ) -> Result<ContentId, StorageError> {
        let provider = self
            .tiers
            .get(tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {tier:?}")))?;

        provider.store(data, metadata).await
    }

    /// Retrieve data by checking tiers in order from hottest to coldest.
    ///
    /// Returns the data from the first tier that has the content.
    pub async fn retrieve_auto(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
        let tier_order = [DataTier::Hot, DataTier::Warm, DataTier::Cold, DataTier::Archive];

        for tier in &tier_order {
            if let Some(provider) = self.tiers.get(tier) {
                match provider.exists(id).await {
                    Ok(true) => return provider.retrieve(id).await,
                    Ok(false) => continue,
                    Err(_) => continue,
                }
            }
        }

        Err(StorageError::NotFound(id.to_string()))
    }

    /// Promote content to a hotter tier.
    ///
    /// Retrieves from the source tier and stores in the destination tier.
    pub async fn promote(
        &self,
        id: &ContentId,
        from_tier: &DataTier,
        to_tier: &DataTier,
        metadata: &Metadata,
    ) -> Result<ContentId, StorageError> {
        let from_provider = self
            .tiers
            .get(from_tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {from_tier:?}")))?;

        let data = from_provider.retrieve(id).await?;

        let to_provider = self
            .tiers
            .get(to_tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {to_tier:?}")))?;

        to_provider.store(&data, metadata).await
    }

    /// Demote content to a colder tier.
    ///
    /// Retrieves from the source tier, stores in the destination tier, then
    /// optionally deletes from the source tier.
    pub async fn demote(
        &self,
        id: &ContentId,
        from_tier: &DataTier,
        to_tier: &DataTier,
        metadata: &Metadata,
        delete_from_source: bool,
    ) -> Result<ContentId, StorageError> {
        let from_provider = self
            .tiers
            .get(from_tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {from_tier:?}")))?;

        let data = from_provider.retrieve(id).await?;

        let to_provider = self
            .tiers
            .get(to_tier)
            .ok_or_else(|| StorageError::ProviderError(format!("no provider for tier {to_tier:?}")))?;

        let new_id = to_provider.store(&data, metadata).await?;

        if delete_from_source {
            // Re-acquire the reference after the previous borrow is done.
            if let Some(provider) = self.tiers.get(from_tier) {
                let _ = provider.delete(id).await;
            }
        }

        Ok(new_id)
    }

    /// Return aggregate stats across all registered tiers.
    pub async fn aggregate_stats(&self) -> Result<StorageStats, StorageError> {
        let mut aggregate = StorageStats::default();

        for provider in self.tiers.values() {
            let s = provider.stats().await?;
            aggregate.total_stored += s.total_stored;
            aggregate.total_retrieved += s.total_retrieved;
            aggregate.total_bytes += s.total_bytes;
            aggregate.cache_hits += s.cache_hits;
            aggregate.cache_misses += s.cache_misses;
        }

        Ok(aggregate)
    }

    /// Return a reference to the tiering configuration.
    pub fn config(&self) -> &TieringConfig {
        &self.tier_config
    }

    /// Verify provenance chain integrity for accepted content.
    pub fn validate_provenance_chain(&self, chain: &ProvenanceChain) -> Result<ProvenanceValidationResult, StorageError> {
        let policy = self.provenance_policy.read()
            .map_err(|e| StorageError::Internal(format!("lock poisoned: {e}")))?
            .clone();
        let validator = ProvenanceValidator::new(policy);
        Ok(validator.validate_chain(chain))
    }

    /// Store content with provenance-first enforcement. Invalid chains are quarantined.
    pub async fn store_verified_with_tier(
        &self,
        tier: &DataTier,
        data: &[u8],
        metadata: &Metadata,
        chain: &ProvenanceChain,
    ) -> Result<(AdmissionDecision, Option<ContentId>), StorageError> {
        let validation = self.validate_provenance_chain(chain)?;
        if validation.valid {
            let id = self.store_with_tier(tier, data, metadata).await?;
            return Ok((AdmissionDecision::Accepted, Some(id)));
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let quarantine_id = format!("q-{}-{}", now, chain.chain_id);
        {
            let mut guard = self.quarantine_records.write()
                .map_err(|e| StorageError::Internal(format!("lock poisoned: {e}")))?;
            guard.insert(
                quarantine_id.clone(),
                QuarantineRecord {
                    quarantine_id: quarantine_id.clone(),
                    reason: validation.reason.clone(),
                    detected_at: now,
                    review_required: true,
                    reinstated: false,
                },
            );
        }

        let quarantine_store = if let Some(provider) = self.tiers.get(&DataTier::Archive) {
            provider.store(data, metadata).await.ok()
        } else {
            None
        };

        Ok((AdmissionDecision::Quarantined, quarantine_store))
    }

    /// Replace the active provenance policy.
    pub fn set_provenance_policy(&self, policy: ProvenancePolicy) -> Result<(), StorageError> {
        let mut guard = self.provenance_policy.write()
            .map_err(|e| StorageError::Internal(format!("lock poisoned: {e}")))?;
        *guard = policy;
        Ok(())
    }

    /// Validate `chain` against the active `ProvenancePolicy` in addition to
    /// structural chain integrity.
    ///
    /// Returns `Ok(())` when the chain satisfies all policy constraints,
    /// otherwise `Err` with a human-readable reason.
    fn validate_chain_with_policy(&self, chain: &ProvenanceChain) -> Result<(), String> {
        let policy = self.provenance_policy.read()
            .map_err(|e| format!("lock poisoned: {e}"))?
            .clone();
        let validator = ProvenanceValidator::new(policy);
        validator.validate_with_policy(chain)
    }

    /// Store data with provenance enforcement according to the active policy.
    ///
    /// * `None` provenance → `Rejected` (when `require_provenance` is true)
    /// * Invalid chain → `Quarantined` with detailed reason
    /// * Valid chain → data stored in `tier`, returns `Accepted`
    pub async fn store_with_provenance_enforcement(
        &self,
        data: &[u8],
        metadata: &Metadata,
        tier: &DataTier,
        provenance_chain: Option<&ProvenanceChain>,
    ) -> Result<AdmissionDecision, StorageError> {
        let require = self.provenance_policy.read()
            .map_err(|e| StorageError::Internal(format!("lock poisoned: {e}")))?
            .require_provenance;

        let chain = match provenance_chain {
            Some(c) => c,
            None => {
                if require {
                    return Ok(AdmissionDecision::Rejected);
                }
                // Policy does not require provenance — store without it.
                self.store_with_tier(tier, data, metadata).await?;
                return Ok(AdmissionDecision::Accepted);
            }
        };

        match self.validate_chain_with_policy(chain) {
            Ok(()) => {
                self.store_with_tier(tier, data, metadata).await?;
                Ok(AdmissionDecision::Accepted)
            }
            Err(reason) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let quarantine_id = format!("q-{}-{}", now, chain.chain_id);
                {
                    let mut guard = self.quarantine_records.write()
                        .map_err(|e| StorageError::Internal(format!("lock poisoned: {e}")))?;
                    guard.insert(
                        quarantine_id.clone(),
                        QuarantineRecord {
                            quarantine_id: quarantine_id.clone(),
                            reason: reason.clone(),
                            detected_at: now,
                            review_required: true,
                            reinstated: false,
                        },
                    );
                }
                Ok(AdmissionDecision::Quarantined)
            }
        }
    }

    /// Store a validated `EnvironmentalReport` with provenance enforcement.
    ///
    /// Steps:
    /// 1. `report.validate()` — structural report validation.
    /// 2. Require `report.provenance` to be `Some`.
    /// 3. Serialize and store with `store_with_provenance_enforcement`.
    pub async fn store_environmental_report(
        &self,
        report: &EnvironmentalReport,
        tier: DataTier,
    ) -> Result<AdmissionDecision, StorageError> {
        // Validate report structure.
        if let Err(reason) = report.validate() {
            return Err(StorageError::InvalidData(reason));
        }

        let provenance = report.provenance.as_ref();

        // Serialize the report to bytes.
        let bytes = serde_json::to_vec(report)
            .map_err(|e| StorageError::InvalidData(format!("report serialization failed: {e}")))?;

        let metadata = Metadata {
            content_type: "application/json".to_string(),
            size: bytes.len() as u64,
            timestamp: report.submitted_at,
            tags: {
                let mut t = std::collections::HashMap::new();
                t.insert("domain".to_string(), format!("{:?}", report.domain));
                t.insert("station_id".to_string(), report.station_id.clone());
                t.insert("report_id".to_string(), report.report_id.clone());
                t
            },
        };

        self.store_with_provenance_enforcement(&bytes, &metadata, &tier, provenance)
            .await
    }

    /// Return quarantine records for synthetic-content review workflows.
    pub fn list_quarantine_records(&self) -> Result<Vec<QuarantineRecord>, StorageError> {
        Ok(self.quarantine_records
            .read()
            .map_err(|e| StorageError::Internal(format!("lock poisoned: {e}")))?
            .values()
            .cloned()
            .collect())
    }

    /// Mark quarantined content as reinstated following review.
    pub fn reinstate_quarantine(&self, quarantine_id: &str) -> Result<bool, StorageError> {
        let mut guard = self.quarantine_records.write()
            .map_err(|e| StorageError::Internal(format!("lock poisoned: {e}")))?;
        let Some(record) = guard.get_mut(quarantine_id) else {
            return Ok(false);
        };
        record.reinstated = true;
        Ok(true)
    }

    /// Configure minimum replicas for a public-interest report class.
    pub fn set_public_interest_policy(&self, class: &str, min_replicas: u8) -> Result<(), StorageError> {
        let mut guard = self.public_interest_policies.write()
            .map_err(|e| StorageError::Internal(format!("lock poisoned: {e}")))?;
        guard.insert(
            class.to_string(),
            PublicInterestAvailabilityPolicy {
                class: class.to_string(),
                min_replicas: min_replicas.max(1),
            },
        );
        Ok(())
    }

    /// Store public-interest content across tiers and compute SLA status.
    pub async fn store_public_interest_with_sla(
        &self,
        class: &str,
        data: &[u8],
        metadata: &Metadata,
        chain: &ProvenanceChain,
    ) -> Result<PublicInterestStoreResult, StorageError> {
        let validation = self.validate_provenance_chain(chain)?;
        if !validation.valid {
            let _ = self
                .store_verified_with_tier(&DataTier::Archive, data, metadata, chain)
                .await?;
            return Err(StorageError::InvalidData(validation.reason));
        }

        let target_replicas = self
            .public_interest_policies
            .read()
            .map_err(|e| StorageError::Internal(format!("lock poisoned: {e}")))?
            .get(class)
            .map(|p| p.min_replicas)
            .unwrap_or(3);

        let tier_order = [DataTier::Hot, DataTier::Warm, DataTier::Cold, DataTier::Archive];
        let mut content_ids = Vec::new();
        for tier in &tier_order {
            if content_ids.len() >= target_replicas as usize {
                break;
            }
            if self.tiers.contains_key(tier) {
                let id = self.store_with_tier(tier, data, metadata).await?;
                content_ids.push(id);
            }
        }

        let replicated = content_ids.len() as u8;
        let sla = AvailabilitySlaStatus {
            class: class.to_string(),
            target_replicas,
            replicated_replicas: replicated,
            read_failover_ready: replicated > 1,
            sla_met: replicated >= target_replicas,
        };

        Ok(PublicInterestStoreResult {
            class: class.to_string(),
            replicated_content_ids: content_ids,
            sla,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use wrfcoin_storage_common::HealthStatus;

    /// A minimal in-memory provider for orchestrator testing.
    struct MockProvider {
        name: String,
        inner: Mutex<MockInner>,
    }

    struct MockInner {
        data: HashMap<String, Vec<u8>>,
        stats: StorageStats,
    }

    impl MockProvider {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                inner: Mutex::new(MockInner {
                    data: HashMap::new(),
                    stats: StorageStats::default(),
                }),
            }
        }
    }

    #[async_trait]
    impl StorageProvider for MockProvider {
        async fn store(
            &self,
            data: &[u8],
            _metadata: &Metadata,
        ) -> Result<ContentId, StorageError> {
            let hash = format!("{:x}", md5_like_hash(data));
            let cid = ContentId {
                provider: self.name.clone(),
                hash: hash.clone(),
            };
            let mut inner = self.inner.lock().unwrap();
            inner.data.insert(hash, data.to_vec());
            inner.stats.total_stored += 1;
            inner.stats.total_bytes += data.len() as u64;
            Ok(cid)
        }

        async fn retrieve(&self, id: &ContentId) -> Result<Vec<u8>, StorageError> {
            let mut inner = self.inner.lock().unwrap();
            match inner.data.get(&id.hash).cloned() {
                Some(d) => {
                    inner.stats.total_retrieved += 1;
                    Ok(d)
                }
                None => Err(StorageError::NotFound(id.to_string())),
            }
        }

        async fn exists(&self, id: &ContentId) -> Result<bool, StorageError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.data.contains_key(&id.hash))
        }

        async fn delete(&self, id: &ContentId) -> Result<(), StorageError> {
            let mut inner = self.inner.lock().unwrap();
            inner.data.remove(&id.hash);
            Ok(())
        }

        async fn health_check(&self) -> Result<HealthStatus, StorageError> {
            Ok(HealthStatus {
                healthy: true,
                provider: self.name.clone(),
                latency_ms: Some(1),
                message: None,
            })
        }

        async fn stats(&self) -> Result<StorageStats, StorageError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.stats.clone())
        }
    }

    /// Simple hash for deterministic test CIDs.
    fn md5_like_hash(data: &[u8]) -> u64 {
        let mut h: u64 = 5381;
        for &b in data {
            h = h.wrapping_mul(33).wrapping_add(b as u64);
        }
        h
    }

    fn test_metadata() -> Metadata {
        Metadata {
            content_type: "application/octet-stream".to_string(),
            size: 0,
            timestamp: 1000,
            tags: HashMap::new(),
        }
    }

    fn valid_chain() -> ProvenanceChain {
        ProvenanceChain {
            chain_id: "chain-1".to_string(),
            root_hash: "root".to_string(),
            links: vec![
                wrfcoin_storage_common::ProvenanceLink {
                    artifact_hash: "root".to_string(),
                    parent_hash: None,
                    signer: "signer-1".to_string(),
                    signature: "sig-1".to_string(),
                    algorithm: "ed25519".to_string(),
                    signed_at: 10,
                },
                wrfcoin_storage_common::ProvenanceLink {
                    artifact_hash: "child-1".to_string(),
                    parent_hash: Some("root".to_string()),
                    signer: "signer-2".to_string(),
                    signature: "sig-2".to_string(),
                    algorithm: "ed25519".to_string(),
                    signed_at: 11,
                },
            ],
        }
    }

    #[tokio::test]
    async fn test_store_with_tier() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        let cid = orch
            .store_with_tier(&DataTier::Hot, b"weather data", &test_metadata())
            .await
            .unwrap();
        assert_eq!(cid.provider, "hot");
    }

    #[tokio::test]
    async fn test_store_no_provider_for_tier() {
        let orch = DataTieringOrchestrator::new(TieringConfig::default());
        let result = orch
            .store_with_tier(&DataTier::Hot, b"data", &test_metadata())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_retrieve_auto_searches_tiers() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        orch.register_tier(DataTier::Cold, Box::new(MockProvider::new("cold")));

        // Store only in Cold tier.
        let cid = orch
            .store_with_tier(&DataTier::Cold, b"cold data", &test_metadata())
            .await
            .unwrap();

        // retrieve_auto should find it even though Hot is checked first.
        let data = orch.retrieve_auto(&cid).await.unwrap();
        assert_eq!(data, b"cold data");
    }

    #[tokio::test]
    async fn test_retrieve_auto_not_found() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        let missing = ContentId {
            provider: "hot".to_string(),
            hash: "nonexistent".to_string(),
        };
        let result = orch.retrieve_auto(&missing).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_promote() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Cold, Box::new(MockProvider::new("cold")));
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        let cold_cid = orch
            .store_with_tier(&DataTier::Cold, b"promote me", &test_metadata())
            .await
            .unwrap();

        let hot_cid = orch
            .promote(&cold_cid, &DataTier::Cold, &DataTier::Hot, &test_metadata())
            .await
            .unwrap();

        assert_eq!(hot_cid.provider, "hot");
        // Data should now be in Hot tier.
        let data = orch
            .store_with_tier(&DataTier::Hot, b"", &test_metadata())
            .await;
        assert!(data.is_ok());
    }

    #[tokio::test]
    async fn test_demote_with_delete() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        orch.register_tier(DataTier::Cold, Box::new(MockProvider::new("cold")));

        let hot_cid = orch
            .store_with_tier(&DataTier::Hot, b"demote me", &test_metadata())
            .await
            .unwrap();

        let cold_cid = orch
            .demote(
                &hot_cid,
                &DataTier::Hot,
                &DataTier::Cold,
                &test_metadata(),
                true,
            )
            .await
            .unwrap();

        assert_eq!(cold_cid.provider, "cold");
    }

    #[tokio::test]
    async fn test_aggregate_stats() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        orch.register_tier(DataTier::Cold, Box::new(MockProvider::new("cold")));

        orch.store_with_tier(&DataTier::Hot, b"aaaa", &test_metadata())
            .await
            .unwrap();
        orch.store_with_tier(&DataTier::Cold, b"bbbbbb", &test_metadata())
            .await
            .unwrap();

        let stats = orch.aggregate_stats().await.unwrap();
        assert_eq!(stats.total_stored, 2);
        assert_eq!(stats.total_bytes, 10); // 4 + 6
    }

    #[tokio::test]
    async fn test_provenance_validation_and_quarantine_flow() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        orch.register_tier(DataTier::Archive, Box::new(MockProvider::new("archive")));

        let invalid = ProvenanceChain {
            chain_id: "chain-bad".to_string(),
            root_hash: "root".to_string(),
            links: vec![wrfcoin_storage_common::ProvenanceLink {
                artifact_hash: "wrong-root".to_string(),
                parent_hash: None,
                signer: "signer".to_string(),
                signature: "sig".to_string(),
                algorithm: "ed25519".to_string(),
                signed_at: 10,
            }],
        };

        let (decision, cid) = orch
            .store_verified_with_tier(&DataTier::Hot, b"bad provenance", &test_metadata(), &invalid)
            .await
            .unwrap();
        assert_eq!(decision, AdmissionDecision::Quarantined);
        assert!(cid.is_some(), "quarantined content should be persisted in archive when available");
        assert_eq!(orch.list_quarantine_records().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_public_interest_sla_replication() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        orch.register_tier(DataTier::Warm, Box::new(MockProvider::new("warm")));
        orch.register_tier(DataTier::Cold, Box::new(MockProvider::new("cold")));
        orch.set_public_interest_policy("public_interest.weather", 3).unwrap();

        let result = orch
            .store_public_interest_with_sla(
                "public_interest.weather",
                b"verified public interest report",
                &test_metadata(),
                &valid_chain(),
            )
            .await
            .unwrap();

        assert_eq!(result.replicated_content_ids.len(), 3);
        assert!(result.sla.sla_met);
        assert!(result.sla.read_failover_ready);
    }

    // ---- store_with_provenance_enforcement tests ----

    #[tokio::test]
    async fn test_provenance_enforcement_valid_chain_accepted() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        let decision = orch
            .store_with_provenance_enforcement(
                b"good data",
                &test_metadata(),
                &DataTier::Hot,
                Some(&valid_chain()),
            )
            .await
            .unwrap();

        assert_eq!(decision, AdmissionDecision::Accepted);
    }

    #[tokio::test]
    async fn test_provenance_enforcement_invalid_chain_quarantined() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        let bad_chain = ProvenanceChain {
            chain_id: "bad-chain".to_string(),
            root_hash: "root".to_string(),
            links: vec![wrfcoin_storage_common::ProvenanceLink {
                artifact_hash: "wrong-hash".to_string(),
                parent_hash: None,
                signer: "signer".to_string(),
                signature: "sig".to_string(),
                algorithm: "ed25519".to_string(),
                signed_at: 1,
            }],
        };

        let decision = orch
            .store_with_provenance_enforcement(
                b"bad data",
                &test_metadata(),
                &DataTier::Hot,
                Some(&bad_chain),
            )
            .await
            .unwrap();

        assert_eq!(decision, AdmissionDecision::Quarantined);
        assert_eq!(orch.list_quarantine_records().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_provenance_enforcement_missing_provenance_rejected() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));
        // Default policy requires provenance.

        let decision = orch
            .store_with_provenance_enforcement(
                b"no provenance",
                &test_metadata(),
                &DataTier::Hot,
                None,
            )
            .await
            .unwrap();

        assert_eq!(decision, AdmissionDecision::Rejected);
    }

    #[tokio::test]
    async fn test_provenance_enforcement_policy_min_chain_length() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        // Require at least 3 links; valid_chain only has 2.
        orch.set_provenance_policy(ProvenancePolicy {
            require_provenance: true,
            min_chain_length: 3,
            allowed_algorithms: Vec::new(),
        }).unwrap();

        let decision = orch
            .store_with_provenance_enforcement(
                b"data",
                &test_metadata(),
                &DataTier::Hot,
                Some(&valid_chain()),
            )
            .await
            .unwrap();

        // Chain is structurally valid but too short — should be quarantined.
        assert_eq!(decision, AdmissionDecision::Quarantined);
    }

    #[tokio::test]
    async fn test_provenance_enforcement_policy_allowed_algorithms() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        // Only allow secp256k1 — valid_chain uses ed25519, so it should fail.
        orch.set_provenance_policy(ProvenancePolicy {
            require_provenance: true,
            min_chain_length: 1,
            allowed_algorithms: vec!["secp256k1".to_string()],
        }).unwrap();

        let decision = orch
            .store_with_provenance_enforcement(
                b"data",
                &test_metadata(),
                &DataTier::Hot,
                Some(&valid_chain()),
            )
            .await
            .unwrap();

        assert_eq!(decision, AdmissionDecision::Quarantined);
    }

    #[tokio::test]
    async fn test_provenance_enforcement_optional_provenance_without_chain_accepted() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        // Disable requirement so None provenance is allowed.
        orch.set_provenance_policy(ProvenancePolicy {
            require_provenance: false,
            min_chain_length: 0,
            allowed_algorithms: Vec::new(),
        }).unwrap();

        let decision = orch
            .store_with_provenance_enforcement(
                b"data without provenance",
                &test_metadata(),
                &DataTier::Hot,
                None,
            )
            .await
            .unwrap();

        assert_eq!(decision, AdmissionDecision::Accepted);
    }

    // ---- store_environmental_report tests ----

    fn sample_env_report_with_provenance() -> wrfcoin_storage_common::EnvironmentalReport {
        use wrfcoin_storage_common::{
            EnvironmentalDomain, EnvironmentalReport, GeoLocation, ProvenanceChain, ReportPayload,
            WeatherPayload,
        };

        EnvironmentalReport {
            report_id: "env-001".to_string(),
            domain: EnvironmentalDomain::Weather,
            station_id: "stn-env-1".to_string(),
            location: GeoLocation { latitude: 45.0, longitude: -90.0, altitude_m: None },
            observed_at: 9_000_000,
            submitted_at: 9_000_001,
            payload: ReportPayload::Weather(WeatherPayload {
                temperature_c: Some(18.0),
                humidity_pct: Some(70.0),
                pressure_hpa: None,
                wind_speed_ms: None,
                wind_direction_deg: None,
                precipitation_mm: None,
                visibility_m: None,
            }),
            quality_score: Some(0.9),
            provenance: Some(ProvenanceChain {
                chain_id: "chain-env".to_string(),
                root_hash: "root".to_string(),
                links: vec![
                    wrfcoin_storage_common::ProvenanceLink {
                        artifact_hash: "root".to_string(),
                        parent_hash: None,
                        signer: "signer-1".to_string(),
                        signature: "sig-1".to_string(),
                        algorithm: "ed25519".to_string(),
                        signed_at: 10,
                    },
                ],
            }),
        }
    }

    #[tokio::test]
    async fn test_store_environmental_report_accepted() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        let report = sample_env_report_with_provenance();
        let decision = orch
            .store_environmental_report(&report, DataTier::Hot)
            .await
            .unwrap();

        assert_eq!(decision, AdmissionDecision::Accepted);
    }

    #[tokio::test]
    async fn test_store_environmental_report_missing_provenance_rejected() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        let mut report = sample_env_report_with_provenance();
        report.provenance = None;

        let decision = orch
            .store_environmental_report(&report, DataTier::Hot)
            .await
            .unwrap();

        assert_eq!(decision, AdmissionDecision::Rejected);
    }

    #[tokio::test]
    async fn test_store_environmental_report_invalid_report_errors() {
        let mut orch = DataTieringOrchestrator::new(TieringConfig::default());
        orch.register_tier(DataTier::Hot, Box::new(MockProvider::new("hot")));

        let mut report = sample_env_report_with_provenance();
        report.report_id = "".to_string(); // triggers validation failure

        let result = orch.store_environmental_report(&report, DataTier::Hot).await;
        assert!(result.is_err());
    }
}
