//! Forecast verification receipt types.

use serde::{Deserialize, Serialize};

/// Accuracy metrics from forecast verification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastAccuracyMetrics {
    /// Mean absolute error.
    pub mae: f64,
    /// Root mean squared error.
    pub rmse: f64,
    /// Mean signed error.
    pub bias: f64,
    /// Skill score normalized to the range [0.0, 1.0].
    pub skill_score: f64,
}

/// Deterministic scoring receipt from forecast verification.
///
/// This struct captures the output of forecast verification, including:
/// - The manifest digest that was verified
/// - The forecast commit identifier
/// - The oracle identifier (ed25519 public key in hex)
/// - The computed accuracy metrics
/// - Verification timestamp and nonce for anti-replay
/// - Optional ZK proof and signature
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastVerificationReceipt {
    /// Manifest digest that was verified (64-char hex).
    pub manifest_digest: String,
    /// Forecast commit identifier.
    pub commit_id: String,
    /// Oracle identifier that produced the receipt (ed25519 pubkey in hex).
    pub oracle_id: String,
    /// Accuracy metrics recorded by verification.
    pub metrics: ForecastAccuracyMetrics,
    /// Verification timestamp as a Unix epoch in seconds.
    pub verified_at: i64,
    /// Anti-replay nonce.
    pub nonce: String,
    /// Optional hex-encoded ZK proof for model execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zk_proof: Option<String>,
    /// Optional receipt signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl ForecastVerificationReceipt {
    /// Create a new forecast verification receipt.
    pub fn new(
        manifest_digest: String,
        commit_id: String,
        oracle_id: String,
        metrics: ForecastAccuracyMetrics,
        verified_at: i64,
        nonce: String,
    ) -> Self {
        Self {
            manifest_digest,
            commit_id,
            oracle_id,
            metrics,
            verified_at,
            nonce,
            zk_proof: None,
            signature: None,
        }
    }

    /// Add an optional ZK proof to the receipt.
    pub fn with_zk_proof(mut self, proof: String) -> Self {
        self.zk_proof = Some(proof);
        self
    }

    /// Add an optional signature to the receipt.
    pub fn with_signature(mut self, sig: String) -> Self {
        self.signature = Some(sig);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_receipt() {
        let metrics = ForecastAccuracyMetrics {
            mae: 0.5,
            rmse: 0.7,
            bias: -0.1,
            skill_score: 0.85,
        };

        let receipt = ForecastVerificationReceipt::new(
            "a".repeat(64),
            "commit-001".to_string(),
            "b".repeat(64),
            metrics.clone(),
            1_000_000,
            "nonce-xyz".to_string(),
        );

        assert_eq!(receipt.manifest_digest.len(), 64);
        assert_eq!(receipt.commit_id, "commit-001");
        assert_eq!(receipt.metrics, metrics);
        assert!(receipt.zk_proof.is_none());
        assert!(receipt.signature.is_none());
    }

    #[test]
    fn test_receipt_with_proof() {
        let metrics = ForecastAccuracyMetrics {
            mae: 0.5,
            rmse: 0.7,
            bias: -0.1,
            skill_score: 0.85,
        };

        let receipt = ForecastVerificationReceipt::new(
            "a".repeat(64),
            "commit-001".to_string(),
            "b".repeat(64),
            metrics,
            1_000_000,
            "nonce-xyz".to_string(),
        )
        .with_zk_proof("proof-data".to_string())
        .with_signature("sig-data".to_string());

        assert_eq!(receipt.zk_proof, Some("proof-data".to_string()));
        assert_eq!(receipt.signature, Some("sig-data".to_string()));
    }

    #[test]
    fn test_receipt_serialization_deserialize() {
        let metrics = ForecastAccuracyMetrics {
            mae: 0.5,
            rmse: 0.7,
            bias: -0.1,
            skill_score: 0.85,
        };

        let receipt = ForecastVerificationReceipt::new(
            "a".repeat(64),
            "commit-001".to_string(),
            "b".repeat(64),
            metrics,
            1_000_000,
            "nonce-xyz".to_string(),
        )
        .with_signature("sig-abc".to_string());

        let json = serde_json::to_value(&receipt).unwrap();
        let deserialized: ForecastVerificationReceipt = serde_json::from_value(json).unwrap();

        assert_eq!(deserialized, receipt);
    }
}
