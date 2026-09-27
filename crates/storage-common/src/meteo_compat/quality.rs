// Vendored compatibility source from wrfcoin/meteo-types commit\n// b27a59fa8a1e8a7db589555ad697260f8a5ae36c. This copy is neutral pending\n// the canonical forecast-score/meteo-types decision.\n\n//! Data quality scoring and classification.

use serde::{Deserialize, Serialize};

/// Errors returned by [`DataQualityScore::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
// Preserve the vendored public variant names for downstream compatibility.
#[allow(clippy::enum_variant_names)]
#[non_exhaustive]
pub enum DataQualityError {
    /// The `overall` score is not finite or not in [0.0, 1.0].
    InvalidOverall,
    /// The `range_score` is not finite or not in [0.0, 1.0].
    InvalidRangeScore,
    /// The `consistency_score` is not finite or not in [0.0, 1.0].
    InvalidConsistencyScore,
}
impl core::fmt::Display for DataQualityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidOverall => write!(f, "overall must be finite and in [0, 1]"),
            Self::InvalidRangeScore => write!(f, "range_score must be finite and in [0, 1]"),
            Self::InvalidConsistencyScore => {
                write!(f, "consistency_score must be finite and in [0, 1]")
            }
        }
    }
}

impl std::error::Error for DataQualityError {}

/// Quality score for a weather observation or data submission.
///
/// Each component is in [0.0, 1.0] where 1.0 is perfect quality.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct DataQualityScore {
    /// Overall quality score (typically a weighted combination of components).
    pub overall: f64,
    /// Score from range validation (are values within physical limits?).
    pub range_score: f64,
    /// Score from temporal/spatial consistency checks.
    pub consistency_score: f64,
}

impl DataQualityScore {
    fn is_component_valid(value: f64) -> bool {
        value.is_finite() && (0.0..=1.0).contains(&value)
    }

    /// Create a new quality score, validating all components.
    ///
    /// Returns [`DataQualityError`] if any component is outside [0.0, 1.0] or
    /// non-finite (NaN, infinity). For trusted/internal paths with guaranteed-valid
    /// values, use [`Self::new_unchecked`].
    pub fn new(
        overall: f64,
        range_score: f64,
        consistency_score: f64,
    ) -> Result<Self, DataQualityError> {
        if !Self::is_component_valid(overall) {
            return Err(DataQualityError::InvalidOverall);
        }
        if !Self::is_component_valid(range_score) {
            return Err(DataQualityError::InvalidRangeScore);
        }
        if !Self::is_component_valid(consistency_score) {
            return Err(DataQualityError::InvalidConsistencyScore);
        }
        Ok(Self {
            overall,
            range_score,
            consistency_score,
        })
    }

    /// Create a new quality score without runtime validation.
    ///
    /// The caller is responsible for ensuring all components are finite and in [0.0, 1.0].
    /// Debug builds will panic on violation. Prefer [`Self::new`] for external input.
    pub fn new_unchecked(overall: f64, range_score: f64, consistency_score: f64) -> Self {
        debug_assert!(
            Self::is_component_valid(overall),
            "overall must be finite and in [0, 1]"
        );
        debug_assert!(
            Self::is_component_valid(range_score),
            "range_score must be finite and in [0, 1]"
        );
        debug_assert!(
            Self::is_component_valid(consistency_score),
            "consistency_score must be finite and in [0, 1]"
        );
        Self {
            overall,
            range_score,
            consistency_score,
        }
    }

    /// Create a new quality score with runtime validation.
    ///
    /// Prefer [`Self::new`] which returns a typed [`DataQualityError`].
    #[must_use = "validation errors must be handled"]
    pub fn try_new(
        overall: f64,
        range_score: f64,
        consistency_score: f64,
    ) -> Result<Self, &'static str> {
        let score = Self {
            overall,
            range_score,
            consistency_score,
        };
        score.validate()?;
        Ok(score)
    }

    /// Return whether all score components are finite and in [0, 1].
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    /// Validate score components.
    #[must_use = "validation errors must be handled"]
    pub fn validate(&self) -> Result<(), &'static str> {
        if !Self::is_component_valid(self.overall) {
            return Err("overall must be finite and in [0, 1]");
        }
        if !Self::is_component_valid(self.range_score) {
            return Err("range_score must be finite and in [0, 1]");
        }
        if !Self::is_component_valid(self.consistency_score) {
            return Err("consistency_score must be finite and in [0, 1]");
        }
        Ok(())
    }

    /// Classify this score into a quality band.
    pub fn band(&self) -> DataQualityBand {
        DataQualityBand::from_score(self.overall)
    }
}

/// Quality classification bands for weather data.
///
/// Thresholds (configurable in production via `DataQualityPolicy`):
/// - High: >= 0.85
/// - Borderline: >= 0.70
/// - WarningOnly: >= 0.50
/// - Low: < 0.50
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
#[non_exhaustive]
pub enum DataQualityBand {
    /// High quality — data is reliable and reward-eligible.
    High,
    /// Borderline — data is usable but may receive reduced rewards.
    Borderline,
    /// Warning only — data is questionable, flagged for review.
    WarningOnly,
    /// Low quality — data is unreliable, may be rejected.
    Low,
}

impl core::fmt::Display for DataQualityBand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::High => write!(f, "High"),
            Self::Borderline => write!(f, "Borderline"),
            Self::WarningOnly => write!(f, "WarningOnly"),
            Self::Low => write!(f, "Low"),
        }
    }
}

impl DataQualityBand {
    /// Classify an overall quality score into a band using default thresholds.
    pub fn from_score(score: f64) -> Self {
        if score >= 0.85 {
            Self::High
        } else if score >= 0.70 {
            Self::Borderline
        } else if score >= 0.50 {
            Self::WarningOnly
        } else {
            Self::Low
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_classification() {
        assert_eq!(DataQualityBand::from_score(0.95), DataQualityBand::High);
        assert_eq!(
            DataQualityBand::from_score(0.75),
            DataQualityBand::Borderline
        );
        assert_eq!(
            DataQualityBand::from_score(0.60),
            DataQualityBand::WarningOnly
        );
        assert_eq!(DataQualityBand::from_score(0.30), DataQualityBand::Low);
    }

    #[test]
    fn score_to_band() {
        let score = DataQualityScore::new_unchecked(0.90, 0.95, 0.85);
        assert_eq!(score.band(), DataQualityBand::High);
    }

    #[test]
    fn try_new_rejects_invalid_values() {
        assert_eq!(
            DataQualityScore::try_new(-0.1, 0.8, 0.9),
            Err("overall must be finite and in [0, 1]")
        );
        assert_eq!(
            DataQualityScore::try_new(0.7, 2.0, 0.9),
            Err("range_score must be finite and in [0, 1]")
        );
        assert_eq!(
            DataQualityScore::try_new(0.7, 0.8, f64::NAN),
            Err("consistency_score must be finite and in [0, 1]")
        );
    }

    #[test]
    fn new_validates_all_components() {
        assert!(DataQualityScore::new(0.8, 0.9, 0.7).is_ok());
        assert_eq!(
            DataQualityScore::new(-0.1, 0.8, 0.9),
            Err(DataQualityError::InvalidOverall)
        );
        assert_eq!(
            DataQualityScore::new(0.8, 1.5, 0.9),
            Err(DataQualityError::InvalidRangeScore)
        );
        assert_eq!(
            DataQualityScore::new(0.8, 0.9, f64::NAN),
            Err(DataQualityError::InvalidConsistencyScore)
        );
    }

    #[test]
    fn new_rejects_nan_and_infinity() {
        assert_eq!(
            DataQualityScore::new(f64::NAN, 0.8, 0.9),
            Err(DataQualityError::InvalidOverall)
        );
        assert_eq!(
            DataQualityScore::new(0.8, f64::INFINITY, 0.9),
            Err(DataQualityError::InvalidRangeScore)
        );
        assert_eq!(
            DataQualityScore::new(0.8, 0.9, f64::NEG_INFINITY),
            Err(DataQualityError::InvalidConsistencyScore)
        );
    }

    #[test]
    fn validate_checks_score_components() {
        let good = DataQualityScore::new_unchecked(0.8, 0.8, 0.8);
        assert!(good.is_valid());

        let bad = DataQualityScore {
            overall: 0.8,
            range_score: 0.8,
            consistency_score: 1.5,
        };
        assert!(!bad.is_valid());
        assert_eq!(
            bad.validate(),
            Err("consistency_score must be finite and in [0, 1]")
        );
    }

    #[test]
    fn boundary_values() {
        assert_eq!(DataQualityBand::from_score(0.85), DataQualityBand::High);
        assert_eq!(
            DataQualityBand::from_score(0.70),
            DataQualityBand::Borderline
        );
        assert_eq!(
            DataQualityBand::from_score(0.50),
            DataQualityBand::WarningOnly
        );
        assert_eq!(DataQualityBand::from_score(0.0), DataQualityBand::Low);
    }

    #[test]
    fn data_quality_error_display() {
        assert_eq!(
            DataQualityError::InvalidOverall.to_string(),
            "overall must be finite and in [0, 1]"
        );
        assert_eq!(
            DataQualityError::InvalidRangeScore.to_string(),
            "range_score must be finite and in [0, 1]"
        );
        assert_eq!(
            DataQualityError::InvalidConsistencyScore.to_string(),
            "consistency_score must be finite and in [0, 1]"
        );
    }

    #[test]
    fn data_quality_band_display() {
        assert_eq!(DataQualityBand::High.to_string(), "High");
        assert_eq!(DataQualityBand::Borderline.to_string(), "Borderline");
        assert_eq!(DataQualityBand::WarningOnly.to_string(), "WarningOnly");
        assert_eq!(DataQualityBand::Low.to_string(), "Low");
    }
}
