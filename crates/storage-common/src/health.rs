//! Health aggregation across multiple storage providers.

use crate::error::StorageError;
use crate::traits::StorageProvider;
use crate::types::HealthStatus;
use std::sync::Arc;

/// Health status for a single named provider.
#[derive(Debug, Clone)]
pub struct ProviderHealthEntry {
    /// Provider name.
    pub name: String,
    /// Health check result (error stringified for clonability).
    pub status: Result<HealthStatus, String>,
}

/// Aggregated health view across all registered providers.
#[derive(Debug, Clone)]
pub struct AggregatedHealth {
    /// `true` when at least one provider is healthy.
    pub overall_healthy: bool,
    /// Per-provider results.
    pub providers: Vec<ProviderHealthEntry>,
    /// Count of healthy providers.
    pub healthy_count: usize,
    /// Total registered providers.
    pub total_count: usize,
}

/// Aggregates health checks from multiple storage providers.
pub struct HealthAggregator {
    providers: Vec<(String, Arc<dyn StorageProvider>)>,
}

impl HealthAggregator {
    /// Create an empty aggregator.
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Register a provider for health monitoring.
    pub fn add_provider(&mut self, name: &str, provider: Arc<dyn StorageProvider>) {
        self.providers.push((name.to_string(), provider));
    }

    /// Run health checks on every registered provider and return an aggregate.
    pub async fn check_all(&self) -> AggregatedHealth {
        let mut entries = Vec::with_capacity(self.providers.len());
        let mut healthy_count: usize = 0;

        for (name, provider) in &self.providers {
            let result = provider.health_check().await;
            let is_healthy = result.as_ref().map_or(false, |h| h.healthy);
            if is_healthy {
                healthy_count += 1;
            }
            entries.push(ProviderHealthEntry {
                name: name.clone(),
                status: result.map_err(|e| e.to_string()),
            });
        }

        AggregatedHealth {
            overall_healthy: healthy_count > 0,
            providers: entries,
            healthy_count,
            total_count: self.providers.len(),
        }
    }

    /// Check a single provider by name.
    pub async fn check_provider(
        &self,
        name: &str,
    ) -> Option<Result<HealthStatus, StorageError>> {
        for (n, provider) in &self.providers {
            if n == name {
                return Some(provider.health_check().await);
            }
        }
        None
    }
}

impl Default for HealthAggregator {
    fn default() -> Self {
        Self::new()
    }
}
