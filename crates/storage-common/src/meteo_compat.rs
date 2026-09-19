//! Vendored compatibility surface from https://github.com/wrfcoin/meteo-types at
//! commit `b27a59fa8a1e8a7db589555ad697260f8a5ae36c`.
//!
//! This copy only removes the sibling-checkout requirement. It does not select the
//! standalone repository or Core4's vendored fork as the canonical type source.

// The source crate has an optional `ts` feature. This compatibility copy intentionally
// carries only Rust/serde behavior, so its dormant upstream cfg attributes remain disabled.
#![allow(unexpected_cfgs)]

mod domain;
mod geo;
mod observation;
mod quality;

pub use domain::{
    AirQualityPayload, EnvironmentalDomain, EnvironmentalReport, HydrologyPayload, OceanPayload,
    ProvenanceChain, ProvenanceEntry, ReportPayload, SoilPayload, WaterQualityPayload,
    WeatherPayload, WildfirePayload,
};
pub use geo::GeoLocation;
pub use observation::WeatherObservation;
pub use quality::{DataQualityBand, DataQualityScore};
