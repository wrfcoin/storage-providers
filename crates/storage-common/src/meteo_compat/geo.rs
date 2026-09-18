// Vendored compatibility source from wrfcoin/meteo-types commit\n// b27a59fa8a1e8a7db589555ad697260f8a5ae36c. This copy is neutral pending\n// the canonical forecast-score/meteo-types decision.\n\n//! Geographic location types.

use serde::{Deserialize, Serialize};

/// A geographic location with optional altitude.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct GeoLocation {
    /// Latitude in decimal degrees (WGS 84). Range: [-90.0, 90.0].
    pub latitude: f64,
    /// Longitude in decimal degrees (WGS 84). Range: [-180.0, 180.0].
    pub longitude: f64,
    /// Altitude above mean sea level in meters. `None` if unknown.
    pub altitude_m: Option<f64>,
}
impl GeoLocation {
    /// Create a new location from latitude and longitude.
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
            altitude_m: None,
        }
    }

    /// Create a new location with altitude.
    pub fn with_altitude(latitude: f64, longitude: f64, altitude_m: f64) -> Self {
        Self {
            latitude,
            longitude,
            altitude_m: Some(altitude_m),
        }
    }

    /// Returns `true` if coordinates are within valid WGS 84 ranges and altitude (if
    /// present) is finite.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        (-90.0..=90.0).contains(&self.latitude)
            && (-180.0..=180.0).contains(&self.longitude)
            && self.altitude_m.is_none_or(|a| a.is_finite())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_location() {
        let loc = GeoLocation::new(40.7128, -74.0060);
        assert!(loc.is_valid());
    }

    #[test]
    fn invalid_latitude() {
        let loc = GeoLocation::new(91.0, 0.0);
        assert!(!loc.is_valid());
    }

    #[test]
    fn with_altitude() {
        let loc = GeoLocation::with_altitude(35.0, 139.0, 40.0);
        assert_eq!(loc.altitude_m, Some(40.0));
    }

    #[test]
    fn invalid_altitude_nan() {
        let loc = GeoLocation::with_altitude(35.0, 139.0, f64::NAN);
        assert!(!loc.is_valid());
    }

    #[test]
    fn invalid_altitude_infinity() {
        let loc = GeoLocation::with_altitude(0.0, 0.0, f64::INFINITY);
        assert!(!loc.is_valid());
    }
}
