//! CF Conventions and NetCDF metadata support for climate data archival standards.
//!
//! Implements Climate and Forecast (CF) conventions for annotating weather and environmental
//! data with standards-compliant metadata. Enables semantic interoperability with scientific
//! data communities and archival systems (THREDDS Data Server, OpenDAP, etc.).
//!
//! Reference: <https://cfconventions.org/cf-conventions/cf-conventions.html>

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// CF-compliant attributes for a single variable.
///
/// Maps a variable name to its CF canonical attributes including standard name,
/// units, cell methods, and coordinate references.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CfVariable {
    /// Variable name in the dataset (e.g., "temperature", "precipitation_flux").
    pub variable_name: String,
    /// CF standard name (e.g., "air_temperature", "lwe_thickness_of_precipitation").
    /// See: <https://cfconventions.org/Data/cf-standard-names/current/build/cf-standard-names.html>
    pub standard_name: String,
    /// Units of measurement (e.g., "K", "kg m-2 s-1").
    /// Should be UDUNITS-compatible: <https://www.unidata.ucar.edu/software/udunits/>
    pub units: String,
    /// Long description of the variable.
    pub long_name: Option<String>,
    /// Cell methods describing how the data was computed over grid cells
    /// (e.g., "time: mean", "area: mean time: maximum").
    pub cell_methods: Option<String>,
    /// Coordinate variables this data depends on (e.g., ["time", "latitude", "longitude"]).
    pub coordinates: Vec<String>,
    /// Valid minimum value (optional, for constraint checking).
    pub valid_min: Option<f64>,
    /// Valid maximum value (optional, for constraint checking).
    pub valid_max: Option<f64>,
    /// Missing value indicator (optional, e.g., -9999.0).
    pub missing_value: Option<f64>,
}

impl CfVariable {
    /// Create a new CF variable with minimal attributes.
    pub fn new(variable_name: String, standard_name: String, units: String) -> Self {
        Self {
            variable_name,
            standard_name,
            units,
            long_name: None,
            cell_methods: None,
            coordinates: vec![],
            valid_min: None,
            valid_max: None,
            missing_value: None,
        }
    }

    /// Set the long name (human-readable description).
    pub fn with_long_name(mut self, long_name: String) -> Self {
        self.long_name = Some(long_name);
        self
    }

    /// Set cell methods (e.g., "time: mean").
    pub fn with_cell_methods(mut self, cell_methods: String) -> Self {
        self.cell_methods = Some(cell_methods);
        self
    }

    /// Set coordinate variables this data depends on.
    pub fn with_coordinates(mut self, coordinates: Vec<String>) -> Self {
        self.coordinates = coordinates;
        self
    }

    /// Set valid minimum value for constraint checking.
    pub fn with_valid_min(mut self, min: f64) -> Self {
        self.valid_min = Some(min);
        self
    }

    /// Set valid maximum value for constraint checking.
    pub fn with_valid_max(mut self, max: f64) -> Self {
        self.valid_max = Some(max);
        self
    }

    /// Set missing value indicator.
    pub fn with_missing_value(mut self, missing: f64) -> Self {
        self.missing_value = Some(missing);
        self
    }
}

/// CF-compliant global attributes for a dataset.
///
/// Encodes dataset-level metadata required by CF conventions (title, history, source, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CfGlobalAttributes {
    /// Dataset title (e.g., "Global Weather Observations - April 2026").
    pub title: String,
    /// Dataset source (e.g., "WRFCoin weather network", "NOAA surface observations").
    pub source: String,
    /// History of processing steps applied to the data.
    pub history: String,
    /// References to relevant scientific papers, standards, or data sources.
    pub references: Option<String>,
    /// Conventions followed (should include "CF-1.10" or appropriate version).
    pub conventions: String,
    /// Institution responsible for the data (e.g., "WRFCoin Foundation").
    pub institution: Option<String>,
    /// Contact information for data provider.
    pub contact: Option<String>,
    /// Comment about the data.
    pub comment: Option<String>,
}

impl CfGlobalAttributes {
    /// Create new global attributes with required CF conventions.
    pub fn new(title: String, source: String, history: String) -> Self {
        Self {
            title,
            source,
            history,
            references: None,
            conventions: "CF-1.10".to_string(),
            institution: None,
            contact: None,
            comment: None,
        }
    }

    /// Set references.
    pub fn with_references(mut self, references: String) -> Self {
        self.references = Some(references);
        self
    }

    /// Set institution.
    pub fn with_institution(mut self, institution: String) -> Self {
        self.institution = Some(institution);
        self
    }

    /// Set contact.
    pub fn with_contact(mut self, contact: String) -> Self {
        self.contact = Some(contact);
        self
    }

    /// Set comment.
    pub fn with_comment(mut self, comment: String) -> Self {
        self.comment = Some(comment);
        self
    }
}

/// CF Conventions metadata annotation for weather data.
///
/// Combines global and variable-level CF attributes to produce
/// standards-compliant metadata suitable for archival and publishing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfMetadata {
    /// Global dataset attributes.
    pub global_attributes: CfGlobalAttributes,
    /// Variables in the dataset and their CF attributes.
    pub variables: HashMap<String, CfVariable>,
}

impl CfMetadata {
    /// Create new CF metadata annotation.
    pub fn new(global_attributes: CfGlobalAttributes) -> Self {
        Self {
            global_attributes,
            variables: HashMap::new(),
        }
    }

    /// Add a variable to the metadata.
    pub fn add_variable(&mut self, var: CfVariable) {
        self.variables.insert(var.variable_name.clone(), var);
    }

    /// Check if a variable is registered.
    pub fn has_variable(&self, name: &str) -> bool {
        self.variables.contains_key(name)
    }

    /// Get a variable by name.
    pub fn get_variable(&self, name: &str) -> Option<&CfVariable> {
        self.variables.get(name)
    }

    /// Convert to key-value tags suitable for storage metadata.
    ///
    /// Produces flattened tags like:
    /// - `cf:global:title` → dataset title
    /// - `cf:global:source` → data source
    /// - `cf:var:temperature:standard_name` → standard CF name
    /// - `cf:var:temperature:units` → variable units
    pub fn to_tags(&self) -> HashMap<String, String> {
        let mut tags = HashMap::new();

        // Global attributes
        tags.insert("cf:global:title".to_string(), self.global_attributes.title.clone());
        tags.insert("cf:global:source".to_string(), self.global_attributes.source.clone());
        tags.insert("cf:global:history".to_string(), self.global_attributes.history.clone());
        tags.insert("cf:global:conventions".to_string(), self.global_attributes.conventions.clone());

        if let Some(ref refs) = self.global_attributes.references {
            tags.insert("cf:global:references".to_string(), refs.clone());
        }
        if let Some(ref inst) = self.global_attributes.institution {
            tags.insert("cf:global:institution".to_string(), inst.clone());
        }
        if let Some(ref contact) = self.global_attributes.contact {
            tags.insert("cf:global:contact".to_string(), contact.clone());
        }
        if let Some(ref comment) = self.global_attributes.comment {
            tags.insert("cf:global:comment".to_string(), comment.clone());
        }

        // Variables
        for (name, var) in &self.variables {
            let prefix = format!("cf:var:{}", name);
            tags.insert(format!("{}:standard_name", prefix), var.standard_name.clone());
            tags.insert(format!("{}:units", prefix), var.units.clone());

            if let Some(ref ln) = var.long_name {
                tags.insert(format!("{}:long_name", prefix), ln.clone());
            }
            if let Some(ref cm) = var.cell_methods {
                tags.insert(format!("{}:cell_methods", prefix), cm.clone());
            }
            if !var.coordinates.is_empty() {
                tags.insert(format!("{}:coordinates", prefix), var.coordinates.join(","));
            }
            if let Some(vmin) = var.valid_min {
                tags.insert(format!("{}:valid_min", prefix), vmin.to_string());
            }
            if let Some(vmax) = var.valid_max {
                tags.insert(format!("{}:valid_max", prefix), vmax.to_string());
            }
            if let Some(mv) = var.missing_value {
                tags.insert(format!("{}:missing_value", prefix), mv.to_string());
            }
        }

        tags
    }
}

/// Registry of standard CF metadata for common weather variables.
///
/// Provides pre-configured CF variables for frequently-stored weather measurements,
/// reducing boilerplate and improving consistency.
pub struct CfRegistry;

impl CfRegistry {
    /// Get CF metadata for air temperature (Kelvin).
    pub fn temperature_k() -> CfVariable {
        CfVariable::new(
            "temperature".to_string(),
            "air_temperature".to_string(),
            "K".to_string(),
        )
        .with_long_name("Air temperature at 2m height".to_string())
        .with_coordinates(vec!["time".to_string(), "latitude".to_string(), "longitude".to_string()])
        .with_valid_min(200.0)
        .with_valid_max(330.0)
    }

    /// Get CF metadata for relative humidity (percentage).
    pub fn relative_humidity() -> CfVariable {
        CfVariable::new(
            "relative_humidity".to_string(),
            "relative_humidity".to_string(),
            "%".to_string(),
        )
        .with_long_name("Relative humidity at 2m height".to_string())
        .with_coordinates(vec!["time".to_string(), "latitude".to_string(), "longitude".to_string()])
        .with_valid_min(0.0)
        .with_valid_max(100.0)
    }

    /// Get CF metadata for precipitation flux (mm/day).
    pub fn precipitation_flux() -> CfVariable {
        CfVariable::new(
            "precipitation_flux".to_string(),
            "lwe_thickness_of_precipitation_amount".to_string(),
            "mm d-1".to_string(),
        )
        .with_long_name("Daily accumulated precipitation".to_string())
        .with_cell_methods("time: sum".to_string())
        .with_coordinates(vec!["time".to_string(), "latitude".to_string(), "longitude".to_string()])
        .with_valid_min(0.0)
    }

    /// Get CF metadata for wind speed (m/s).
    pub fn wind_speed() -> CfVariable {
        CfVariable::new(
            "wind_speed".to_string(),
            "wind_speed".to_string(),
            "m s-1".to_string(),
        )
        .with_long_name("Wind speed at 10m height".to_string())
        .with_coordinates(vec!["time".to_string(), "latitude".to_string(), "longitude".to_string()])
        .with_valid_min(0.0)
    }

    /// Get CF metadata for atmospheric pressure (Pa).
    pub fn surface_pressure() -> CfVariable {
        CfVariable::new(
            "surface_pressure".to_string(),
            "surface_air_pressure".to_string(),
            "Pa".to_string(),
        )
        .with_long_name("Surface air pressure".to_string())
        .with_coordinates(vec!["time".to_string(), "latitude".to_string(), "longitude".to_string()])
        .with_valid_min(80000.0)
        .with_valid_max(115000.0)
    }

    /// Get CF metadata for solar shortwave radiation (W/m²).
    pub fn shortwave_radiation() -> CfVariable {
        CfVariable::new(
            "shortwave_radiation".to_string(),
            "surface_downwelling_shortwave_flux_in_air".to_string(),
            "W m-2".to_string(),
        )
        .with_long_name("Downwelling solar radiation at surface".to_string())
        .with_cell_methods("time: mean".to_string())
        .with_coordinates(vec!["time".to_string(), "latitude".to_string(), "longitude".to_string()])
        .with_valid_min(0.0)
        .with_valid_max(1500.0)
    }

    /// Get CF metadata for soil moisture (m³/m³).
    pub fn soil_moisture() -> CfVariable {
        CfVariable::new(
            "soil_moisture".to_string(),
            "soil_moisture_content".to_string(),
            "m3 m-3".to_string(),
        )
        .with_long_name("Soil volumetric water content".to_string())
        .with_coordinates(vec!["time".to_string(), "depth".to_string(), "latitude".to_string(), "longitude".to_string()])
        .with_valid_min(0.0)
        .with_valid_max(1.0)
    }

    /// Get CF metadata for air quality PM2.5 (µg/m³).
    pub fn pm25_concentration() -> CfVariable {
        CfVariable::new(
            "pm25".to_string(),
            "mass_concentration_of_pm2p5_aerosol_in_air".to_string(),
            "µg m-3".to_string(),
        )
        .with_long_name("PM2.5 mass concentration".to_string())
        .with_coordinates(vec!["time".to_string(), "latitude".to_string(), "longitude".to_string()])
        .with_valid_min(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cf_variable_creation() {
        let var = CfVariable::new(
            "temp".to_string(),
            "air_temperature".to_string(),
            "K".to_string(),
        );
        assert_eq!(var.variable_name, "temp");
        assert_eq!(var.standard_name, "air_temperature");
        assert_eq!(var.units, "K");
    }

    #[test]
    fn test_cf_variable_builder() {
        let var = CfRegistry::temperature_k();
        assert_eq!(var.standard_name, "air_temperature");
        assert_eq!(var.units, "K");
        assert!(var.long_name.is_some());
        assert_eq!(var.coordinates.len(), 3);
        assert_eq!(var.valid_min, Some(200.0));
        assert_eq!(var.valid_max, Some(330.0));
    }

    #[test]
    fn test_cf_global_attributes() {
        let attrs = CfGlobalAttributes::new(
            "Test Dataset".to_string(),
            "WRFCoin Network".to_string(),
            "Created 2026-04-09".to_string(),
        );
        assert_eq!(attrs.title, "Test Dataset");
        assert_eq!(attrs.conventions, "CF-1.10");
    }

    #[test]
    fn test_cf_metadata_add_variable() {
        let mut cf = CfMetadata::new(CfGlobalAttributes::new(
            "Test".to_string(),
            "Test".to_string(),
            "Test".to_string(),
        ));
        cf.add_variable(CfRegistry::temperature_k());
        assert!(cf.has_variable("temperature"));
        assert_eq!(cf.variables.len(), 1);
    }

    #[test]
    fn test_cf_metadata_to_tags() {
        let mut cf = CfMetadata::new(
            CfGlobalAttributes::new(
                "Test Dataset".to_string(),
                "WRFCoin".to_string(),
                "Test".to_string(),
            )
            .with_institution("WRFC Foundation".to_string()),
        );
        cf.add_variable(CfRegistry::temperature_k());

        let tags = cf.to_tags();
        assert!(tags.contains_key("cf:global:title"));
        assert_eq!(tags.get("cf:global:title"), Some(&"Test Dataset".to_string()));
        assert!(tags.contains_key("cf:global:institution"));
        assert!(tags.contains_key("cf:var:temperature:standard_name"));
        assert_eq!(tags.get("cf:var:temperature:units"), Some(&"K".to_string()));
    }

    #[test]
    fn test_cf_variable_serde() {
        let var = CfRegistry::precipitation_flux();
        let json = serde_json::to_string(&var).expect("serialize");
        let restored: CfVariable = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(var, restored);
    }

    #[test]
    fn test_cf_metadata_serde() {
        let mut cf = CfMetadata::new(CfGlobalAttributes::new(
            "Test".to_string(),
            "Test".to_string(),
            "Test".to_string(),
        ));
        cf.add_variable(CfRegistry::wind_speed());

        let json = serde_json::to_string(&cf).expect("serialize");
        let restored: CfMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cf.global_attributes.title, restored.global_attributes.title);
        assert!(restored.has_variable("wind_speed"));
    }

    #[test]
    fn test_cf_registry_standard_variables() {
        let temp = CfRegistry::temperature_k();
        let humidity = CfRegistry::relative_humidity();
        let precip = CfRegistry::precipitation_flux();
        let wind = CfRegistry::wind_speed();
        let pressure = CfRegistry::surface_pressure();
        let radiation = CfRegistry::shortwave_radiation();
        let soil = CfRegistry::soil_moisture();
        let pm25 = CfRegistry::pm25_concentration();

        // Verify all standard variables are correctly configured
        assert_eq!(temp.standard_name, "air_temperature");
        assert_eq!(humidity.standard_name, "relative_humidity");
        assert_eq!(precip.standard_name, "lwe_thickness_of_precipitation_amount");
        assert_eq!(wind.standard_name, "wind_speed");
        assert_eq!(pressure.standard_name, "surface_air_pressure");
        assert_eq!(radiation.standard_name, "surface_downwelling_shortwave_flux_in_air");
        assert_eq!(soil.standard_name, "soil_moisture_content");
        assert_eq!(pm25.standard_name, "mass_concentration_of_pm2p5_aerosol_in_air");

        // All should have coordinates and valid constraints
        assert!(!temp.coordinates.is_empty());
        assert!(!precip.coordinates.is_empty());
        assert!(soil.valid_min.is_some());
        assert!(pm25.valid_min.is_some());
    }

    #[test]
    fn test_cf_coordinates_integration() {
        let mut cf = CfMetadata::new(CfGlobalAttributes::new(
            "Multi-var".to_string(),
            "WRFCoin".to_string(),
            "Test".to_string(),
        ));

        cf.add_variable(CfRegistry::temperature_k());
        cf.add_variable(CfRegistry::relative_humidity());
        cf.add_variable(CfRegistry::precipitation_flux());

        let tags = cf.to_tags();

        // Verify all variables' coordinates are in tags
        assert!(tags.get("cf:var:temperature:coordinates").is_some());
        assert!(tags.get("cf:var:relative_humidity:coordinates").is_some());
        assert!(tags.get("cf:var:precipitation_flux:coordinates").is_some());
        assert!(tags.get("cf:var:precipitation_flux:cell_methods").is_some());
    }

    #[test]
    fn test_cf_variable_immutable_constraints() {
        let var = CfRegistry::temperature_k()
            .with_valid_min(273.0)
            .with_valid_max(320.0)
            .with_missing_value(-999.0);

        assert_eq!(var.valid_min, Some(273.0));
        assert_eq!(var.valid_max, Some(320.0));
        assert_eq!(var.missing_value, Some(-999.0));
    }
}
