// Vendored compatibility source from wrfcoin/meteo-types commit\n// b27a59fa8a1e8a7db589555ad697260f8a5ae36c. This copy is neutral pending\n// the canonical forecast-score/meteo-types decision.\n\n//! Core weather observation types.

use serde::{Deserialize, Serialize};

/// Whether the `pressure_hpa` field represents station-level or sea-level-corrected pressure.
///
/// Altitude-aware validation (cross-station pressure comparison, QFF reduction) requires
/// knowing which reference level was used. Without this discriminator the validator cannot
/// determine whether a pressure reading is anomalous or simply at a high-altitude station.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
#[non_exhaustive]
pub enum PressureType {
    /// Raw sensor reading at station altitude (QFE).
    StationLevel,
    /// Reduced to mean sea level (QNH / QFF).
    MeanSeaLevel,
    /// Legacy data or unspecified reduction method.
    #[default]
    Unknown,
}
/// Shelter/housing type for a temperature/humidity sensor.
///
/// Passive radiation shields can read 2–5 °C high during calm, sunny conditions due to
/// radiative heating (WMO-No. 8, Ch. 2). The shelter type must be recorded so quality
/// scoring can apply the appropriate radiation-error correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
#[non_exhaustive]
pub enum ShelterType {
    /// Standard Stevenson screen (WMO-compliant louvred white box).
    StevensonScreen,
    /// Actively aspirated radiation shield (research / ASOS grade).
    AspiratedShield,
    /// Passive radiation shield (Davis, ambient weather, most consumer stations).
    PassiveShield,
    /// No shelter — sensor fully exposed to direct radiation.
    Exposed,
    /// Shelter type unknown or not reported.
    Unknown,
}

/// Metadata about the sensor hardware that produced an observation.
///
/// Links a `WeatherObservation` to its physical sensor characteristics so that quality
/// scoring can weight readings according to hardware accuracy class.
///
/// Boxed in `WeatherObservation` to keep the core struct small — most code paths that
/// do not need hardware weighting can skip allocation entirely.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct SensorMetadata {
    /// Manufacturer/model string, lower-kebab-case (e.g. `"bme280"`, `"vaisala-hmp155"`).
    pub hardware_model: Option<String>,
    /// Unix timestamp (seconds) of the last calibration event. `None` = unknown.
    pub last_calibrated_at: Option<i64>,
    /// Shelter/housing type for temperature and humidity sensors.
    pub shelter_type: Option<ShelterType>,
}

/// A weather observation with standard meteorological variables.
///
/// All measurement fields are optional — sensors may not report every variable.
/// Units follow SI / WMO conventions.
///
/// # New fields (issue #23)
///
/// | Field | Unit | Notes |
/// |-------|------|-------|
/// | `solar_radiation_wm2` | W/m² | Global horizontal irradiance for diurnal QC |
/// | `cloud_cover_oktas` | oktas [0, 8] | For radiation consistency checks |
/// | `wind_gust_ms` | m/s | Max 3-second average; must be ≥ `wind_speed_ms` |
/// | `observation_period_seconds` | s | Makes precipitation accumulation unambiguous |
/// | `pressure_type` | `PressureType` | Distinguishes QFE from QFF/QNH |
/// | `sensor_metadata` | `SensorMetadata` | Hardware-aware quality weighting |
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct WeatherObservation {
    /// Air temperature in degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Relative humidity as a percentage [0, 100].
    pub humidity_percent: Option<f64>,
    /// Atmospheric pressure in hectopascals (hPa / mbar).
    pub pressure_hpa: Option<f64>,
    /// Whether `pressure_hpa` is station-level, sea-level-corrected, or unspecified.
    pub pressure_type: PressureType,
    /// Wind speed (10-minute average) in meters per second.
    pub wind_speed_ms: Option<f64>,
    /// Wind direction in degrees [0, 360). 0 = North, 90 = East.
    pub wind_direction_deg: Option<f64>,
    /// Peak wind gust (maximum 3-second average) in meters per second.
    ///
    /// The gust-to-mean ratio (gust / `wind_speed_ms`) should normally be < 3.0.
    /// Ratios exceeding this in stable conditions suggest sensor malfunction.
    pub wind_gust_ms: Option<f64>,
    /// Precipitation accumulation in millimeters over `observation_period_seconds`.
    pub precipitation_mm: Option<f64>,
    /// Duration of the observation period in seconds.
    ///
    /// Required for unambiguous precipitation rate interpretation: 10 mm in 60 s is a
    /// torrential downburst; 10 mm in 86 400 s is light rain.
    pub observation_period_seconds: Option<u64>,
    /// Dewpoint temperature in degrees Celsius.
    pub dewpoint_c: Option<f64>,
    /// Horizontal visibility in meters.
    pub visibility_m: Option<f64>,
    /// Global horizontal irradiance (GHI) in W/m².
    ///
    /// Range: [0, ~1400]. The most important diagnostic for detecting stuck sensors and
    /// validating the diurnal temperature cycle (NOAA ASOS/AWOS; WMO SYNOP Sec. 3).
    pub solar_radiation_wm2: Option<f64>,
    /// Cloud cover in oktas [0, 8].
    ///
    /// Used for solar radiation consistency checks and sky condition reporting.
    pub cloud_cover_oktas: Option<u8>,
    /// Optional hardware metadata for quality weighting.
    ///
    /// `Box`ed to keep the common (no-metadata) path allocation-free.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensor_metadata: Option<Box<SensorMetadata>>,
}

impl WeatherObservation {
    /// Returns `true` if all present values fall within physically plausible ranges.
    ///
    /// Ranges based on observed Earth records:
    /// - Temperature: -89.2 °C (Vostok) to 56.7 °C (Death Valley)
    /// - Humidity: 0–100 %
    /// - Pressure: 870–1084 hPa (observed extremes)
    /// - Wind speed: 0–113 m/s (strongest recorded gust)
    /// - Wind gust: 0–113 m/s; must be ≥ wind_speed_ms when both present
    /// - Wind direction: 0–360°
    /// - Precipitation: 0–500 mm
    /// - Dewpoint: -80 °C to 35 °C
    /// - Visibility: 0–100,000 m
    /// - Solar radiation: 0–1,400 W/m²
    /// - Cloud cover: 0–8 oktas
    #[must_use]
    pub fn is_physically_plausible(&self) -> bool {
        let basic_checks = [
            self.temperature_c.map(|v| (-89.2..=56.7).contains(&v)),
            self.humidity_percent.map(|v| (0.0..=100.0).contains(&v)),
            self.pressure_hpa.map(|v| (870.0..=1084.0).contains(&v)),
            self.wind_speed_ms.map(|v| (0.0..=113.0).contains(&v)),
            self.wind_direction_deg.map(|v| (0.0..360.0).contains(&v)),
            self.wind_gust_ms.map(|v| (0.0..=113.0).contains(&v)),
            self.precipitation_mm.map(|v| (0.0..=500.0).contains(&v)),
            self.dewpoint_c.map(|v| (-80.0..=35.0).contains(&v)),
            self.visibility_m.map(|v| (0.0..=100_000.0).contains(&v)),
            self.solar_radiation_wm2.map(|v| (0.0..=1400.0).contains(&v)),
            self.cloud_cover_oktas.map(|v| v <= 8),
        ];
        if !basic_checks.iter().all(|c| c.unwrap_or(true)) {
            return false;
        }
        // Gust must be >= mean wind speed when both are present.
        if let (Some(gust), Some(speed)) = (self.wind_gust_ms, self.wind_speed_ms) {
            if gust < speed {
                return false;
            }
        }
        true
    }

    /// Count how many measurement variables are present (non-None).
    ///
    /// Counts numeric measurement fields only. `pressure_type` and `sensor_metadata` are
    /// metadata and are not counted.
    pub fn variable_count(&self) -> usize {
        [
            self.temperature_c.is_some(),
            self.humidity_percent.is_some(),
            self.pressure_hpa.is_some(),
            self.wind_speed_ms.is_some(),
            self.wind_direction_deg.is_some(),
            self.wind_gust_ms.is_some(),
            self.precipitation_mm.is_some(),
            self.observation_period_seconds.is_some(),
            self.dewpoint_c.is_some(),
            self.visibility_m.is_some(),
            self.solar_radiation_wm2.is_some(),
            self.cloud_cover_oktas.is_some(),
        ]
        .iter()
        .filter(|&&v| v)
        .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let obs = WeatherObservation::default();
        assert_eq!(obs.variable_count(), 0);
        assert!(obs.is_physically_plausible());
        assert_eq!(obs.pressure_type, PressureType::Unknown);
    }

    #[test]
    fn plausible_observation() {
        let obs = WeatherObservation {
            temperature_c: Some(22.5),
            humidity_percent: Some(65.0),
            pressure_hpa: Some(1013.25),
            pressure_type: PressureType::MeanSeaLevel,
            wind_speed_ms: Some(5.0),
            wind_direction_deg: Some(180.0),
            wind_gust_ms: Some(8.0),
            precipitation_mm: Some(0.0),
            observation_period_seconds: Some(3600),
            dewpoint_c: Some(15.0),
            visibility_m: Some(10_000.0),
            solar_radiation_wm2: Some(650.0),
            cloud_cover_oktas: Some(3),
            sensor_metadata: None,
        };
        assert!(obs.is_physically_plausible());
        assert_eq!(obs.variable_count(), 12);
    }

    #[test]
    fn implausible_temperature() {
        let obs = WeatherObservation {
            temperature_c: Some(100.0), // impossible
            ..Default::default()
        };
        assert!(!obs.is_physically_plausible());
    }

    #[test]
    fn implausible_solar_radiation() {
        let obs = WeatherObservation {
            solar_radiation_wm2: Some(2000.0), // above 1400 W/m²
            ..Default::default()
        };
        assert!(!obs.is_physically_plausible());
    }

    #[test]
    fn implausible_cloud_cover_oktas() {
        let obs = WeatherObservation {
            cloud_cover_oktas: Some(9), // oktas max is 8
            ..Default::default()
        };
        assert!(!obs.is_physically_plausible());
    }

    #[test]
    fn gust_must_be_at_least_mean_speed() {
        let obs = WeatherObservation {
            wind_speed_ms: Some(10.0),
            wind_gust_ms: Some(5.0), // gust < mean — impossible
            ..Default::default()
        };
        assert!(!obs.is_physically_plausible());
    }

    #[test]
    fn gust_equals_mean_speed_is_ok() {
        let obs = WeatherObservation {
            wind_speed_ms: Some(10.0),
            wind_gust_ms: Some(10.0),
            ..Default::default()
        };
        assert!(obs.is_physically_plausible());
    }

    #[test]
    fn partial_observation() {
        let obs = WeatherObservation {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            ..Default::default()
        };
        assert_eq!(obs.variable_count(), 2);
        assert!(obs.is_physically_plausible());
    }

    #[test]
    fn serde_roundtrip_extended() {
        let obs = WeatherObservation {
            temperature_c: Some(22.5),
            humidity_percent: Some(65.0),
            pressure_hpa: Some(1013.25),
            pressure_type: PressureType::MeanSeaLevel,
            wind_speed_ms: Some(5.0),
            wind_direction_deg: Some(180.0),
            wind_gust_ms: Some(8.0),
            precipitation_mm: Some(0.0),
            observation_period_seconds: Some(3600),
            dewpoint_c: Some(15.0),
            visibility_m: Some(10_000.0),
            solar_radiation_wm2: Some(650.0),
            cloud_cover_oktas: Some(3),
            sensor_metadata: Some(Box::new(SensorMetadata {
                hardware_model: Some("vaisala-hmp155".to_string()),
                last_calibrated_at: Some(1_700_000_000),
                shelter_type: Some(ShelterType::AspiratedShield),
            })),
        };
        let json = serde_json::to_string(&obs).unwrap();
        let deserialized: WeatherObservation = serde_json::from_str(&json).unwrap();
        assert_eq!(obs, deserialized);
    }

    #[test]
    fn sensor_metadata_skipped_when_none() {
        let obs = WeatherObservation {
            temperature_c: Some(20.0),
            ..Default::default()
        };
        let json = serde_json::to_string(&obs).unwrap();
        assert!(!json.contains("sensor_metadata"));
    }

    #[test]
    fn pressure_type_default_is_unknown() {
        let pt: PressureType = Default::default();
        assert_eq!(pt, PressureType::Unknown);
    }

    #[test]
    fn shelter_type_serde_roundtrip() {
        let s = ShelterType::PassiveShield;
        let json = serde_json::to_string(&s).unwrap();
        let back: ShelterType = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
