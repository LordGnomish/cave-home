//! Port of `homeassistant.config` + `homeassistant.core_config`.
//!
//! Two pieces live here. [`Schema`] is a small voluptuous-style validator —
//! HA validates every YAML block against a `vol.Schema`; this models the
//! subset that matters (typed keys, required/optional, defaults, extra-key
//! policy) and validates a parsed `serde_yaml::Value` mapping. [`CoreConfig`]
//! is the typed `homeassistant:` block (name, lat/long/elevation, unit system,
//! time zone, currency, …) parsed and range-validated via that schema.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("invalid YAML: {0}")]
    Yaml(String),
    #[error("config is not a mapping")]
    NotAMapping,
    #[error("validation failed: {0:?}")]
    Validation(Vec<ValidationError>),
}

/// A single field-level validation failure (key + human reason).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub key: String,
    pub message: String,
}

impl ValidationError {
    fn new(key: impl Into<String>, message: impl Into<String>) -> Self {
        Self { key: key.into(), message: message.into() }
    }
}

/// The scalar types a [`Field`] can require (HA's `cv.string` / `cv.positive_int`
/// / `cv.boolean` / numeric coercers, reduced to four shapes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Str,
    Int,
    Float,
    Bool,
}

/// One key in a [`Schema`].
#[derive(Clone, Debug)]
pub struct Field {
    pub key: String,
    pub ty: FieldType,
    pub required: bool,
    pub default: Option<serde_yaml::Value>,
}

impl Field {
    #[must_use]
    pub fn required(key: impl Into<String>, ty: FieldType) -> Self {
        Self { key: key.into(), ty, required: true, default: None }
    }

    #[must_use]
    pub fn optional(key: impl Into<String>, ty: FieldType) -> Self {
        Self { key: key.into(), ty, required: false, default: None }
    }

    #[must_use]
    pub fn with_default(mut self, value: serde_yaml::Value) -> Self {
        self.default = Some(value);
        self
    }
}

/// Port of a `vol.Schema` over a YAML mapping.
#[derive(Clone, Debug, Default)]
pub struct Schema {
    pub fields: Vec<Field>,
    /// When false, keys not described by a [`Field`] are a validation error
    /// (HA's `PREVENT_EXTRA`); when true they pass through (`ALLOW_EXTRA`).
    pub allow_extra: bool,
}

impl Schema {
    #[must_use]
    pub fn new(fields: Vec<Field>) -> Self {
        Self { fields, allow_extra: false }
    }

    #[must_use]
    pub fn allow_extra(mut self) -> Self {
        self.allow_extra = true;
        self
    }

    /// Validate `value` (which must be a YAML mapping). Returns the normalised
    /// map with defaults applied, or every collected [`ValidationError`].
    ///
    /// # Errors
    /// [`ConfigError::NotAMapping`] if `value` is not a mapping;
    /// [`ConfigError::Validation`] with all field failures otherwise.
    pub fn validate(
        &self,
        value: &serde_yaml::Value,
    ) -> Result<BTreeMap<String, serde_yaml::Value>, ConfigError> {
        let _ = value;
        unimplemented!("RED")
    }
}

/// Port of `homeassistant.core_config.UnitSystem` selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitSystem {
    #[default]
    Metric,
    UsCustomary,
}

/// Port of the typed `homeassistant:` core config block.
#[derive(Clone, Debug, PartialEq)]
pub struct CoreConfig {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: i64,
    pub unit_system: UnitSystem,
    pub time_zone: String,
    pub currency: String,
    pub country: Option<String>,
    pub language: String,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            name: "Home".to_owned(),
            latitude: 0.0,
            longitude: 0.0,
            elevation: 0,
            unit_system: UnitSystem::Metric,
            time_zone: "UTC".to_owned(),
            currency: "USD".to_owned(),
            country: None,
            language: "en".to_owned(),
        }
    }
}

impl CoreConfig {
    /// Parse and validate a `homeassistant:` core-config YAML block. Missing
    /// keys fall back to [`CoreConfig::default`]; `latitude`/`longitude` are
    /// range-checked and `unit_system` must be a known token (with `imperial`
    /// accepted as an alias for `us_customary`, mirroring HA's rename).
    ///
    /// # Errors
    /// [`ConfigError::Yaml`] on a parse error, [`ConfigError::NotAMapping`] if
    /// the block is not a mapping, [`ConfigError::Validation`] on any bad value.
    pub fn from_yaml(yaml: &str) -> Result<Self, ConfigError> {
        let _ = yaml;
        unimplemented!("RED")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Value;

    fn parse(yaml: &str) -> Value {
        serde_yaml::from_str(yaml).expect("yaml")
    }

    #[test]
    fn schema_applies_defaults_and_checks_required() {
        let schema = Schema::new(vec![
            Field::required("name", FieldType::Str),
            Field::optional("port", FieldType::Int).with_default(Value::from(8123)),
        ]);
        let out = schema.validate(&parse("name: cave")).expect("valid");
        assert_eq!(out["name"], Value::from("cave"));
        // default applied for the missing optional
        assert_eq!(out["port"], Value::from(8123));

        // missing required -> error
        let err = schema.validate(&parse("port: 80")).unwrap_err();
        assert_eq!(
            err,
            ConfigError::Validation(vec![ValidationError::new("name", "required key is missing")])
        );
    }

    #[test]
    fn schema_type_mismatch_is_reported() {
        let schema = Schema::new(vec![Field::required("port", FieldType::Int)]);
        let err = schema.validate(&parse("port: not_a_number")).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(ref v) if v[0].key == "port"));
    }

    #[test]
    fn schema_extra_keys_rejected_unless_allowed() {
        let schema = Schema::new(vec![Field::optional("a", FieldType::Str)]);
        let err = schema.validate(&parse("a: x\nb: y")).unwrap_err();
        assert_eq!(
            err,
            ConfigError::Validation(vec![ValidationError::new("b", "extra key not allowed")])
        );
        // allow_extra passes the unknown key through
        let lax = Schema::new(vec![Field::optional("a", FieldType::Str)]).allow_extra();
        let out = lax.validate(&parse("a: x\nb: y")).expect("valid");
        assert_eq!(out["b"], Value::from("y"));
    }

    #[test]
    fn schema_float_accepts_integer_input() {
        let schema = Schema::new(vec![Field::required("lat", FieldType::Float)]);
        let out = schema.validate(&parse("lat: 52")).expect("valid");
        assert_eq!(out["lat"].as_f64(), Some(52.0));
    }

    #[test]
    fn schema_rejects_non_mapping() {
        let schema = Schema::new(vec![]);
        assert_eq!(schema.validate(&parse("- 1\n- 2")).unwrap_err(), ConfigError::NotAMapping);
    }

    #[test]
    fn core_config_parses_and_defaults() {
        let cfg = CoreConfig::from_yaml(
            "name: Cave\nlatitude: 52.37\nlongitude: 4.90\nelevation: 3\nunit_system: metric\ntime_zone: Europe/Amsterdam",
        )
        .expect("config");
        assert_eq!(cfg.name, "Cave");
        assert!((cfg.latitude - 52.37).abs() < 1e-9);
        assert_eq!(cfg.elevation, 3);
        assert_eq!(cfg.unit_system, UnitSystem::Metric);
        assert_eq!(cfg.time_zone, "Europe/Amsterdam");
        // unspecified keys fall back to defaults
        assert_eq!(cfg.currency, "USD");
        assert_eq!(cfg.language, "en");
        assert!(cfg.country.is_none());
    }

    #[test]
    fn core_config_empty_block_is_all_defaults() {
        let cfg = CoreConfig::from_yaml("{}").expect("config");
        assert_eq!(cfg, CoreConfig::default());
    }

    #[test]
    fn core_config_imperial_aliases_us_customary() {
        let cfg = CoreConfig::from_yaml("unit_system: imperial").expect("config");
        assert_eq!(cfg.unit_system, UnitSystem::UsCustomary);
        let cfg2 = CoreConfig::from_yaml("unit_system: us_customary").expect("config");
        assert_eq!(cfg2.unit_system, UnitSystem::UsCustomary);
    }

    #[test]
    fn core_config_rejects_out_of_range_coordinates() {
        let err = CoreConfig::from_yaml("latitude: 999\nlongitude: 4.0").unwrap_err();
        assert!(matches!(err, ConfigError::Validation(ref v) if v.iter().any(|e| e.key == "latitude")));
        let err2 = CoreConfig::from_yaml("longitude: -500").unwrap_err();
        assert!(matches!(err2, ConfigError::Validation(ref v) if v.iter().any(|e| e.key == "longitude")));
    }

    #[test]
    fn core_config_rejects_unknown_unit_system() {
        let err = CoreConfig::from_yaml("unit_system: furlongs").unwrap_err();
        assert!(matches!(err, ConfigError::Validation(ref v) if v[0].key == "unit_system"));
    }

    #[test]
    fn core_config_bad_yaml_errors() {
        assert!(matches!(CoreConfig::from_yaml("name: : :").unwrap_err(), ConfigError::Yaml(_)));
    }
}
