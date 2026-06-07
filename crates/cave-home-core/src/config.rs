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
    pub const fn new(fields: Vec<Field>) -> Self {
        Self { fields, allow_extra: false }
    }

    #[must_use]
    pub const fn allow_extra(mut self) -> Self {
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
        let mapping = value.as_mapping().ok_or(ConfigError::NotAMapping)?;
        let mut out = BTreeMap::new();
        let mut errors = Vec::new();

        // Validate each declared field.
        for field in &self.fields {
            match mapping.get(serde_yaml::Value::from(field.key.as_str())) {
                Some(raw) => match coerce(field.ty, raw) {
                    Some(coerced) => {
                        out.insert(field.key.clone(), coerced);
                    }
                    None => errors.push(ValidationError::new(
                        &field.key,
                        format!("expected {:?}", field.ty),
                    )),
                },
                None => {
                    if let Some(default) = &field.default {
                        out.insert(field.key.clone(), default.clone());
                    } else if field.required {
                        errors.push(ValidationError::new(&field.key, "required key is missing"));
                    }
                }
            }
        }

        // Reject (or pass through) keys not described by any field.
        let known: std::collections::HashSet<&str> =
            self.fields.iter().map(|f| f.key.as_str()).collect();
        for (k, v) in mapping {
            let Some(key) = k.as_str() else { continue };
            if known.contains(key) {
                continue;
            }
            if self.allow_extra {
                out.insert(key.to_owned(), v.clone());
            } else {
                errors.push(ValidationError::new(key, "extra key not allowed"));
            }
        }

        if errors.is_empty() {
            Ok(out)
        } else {
            // Stable ordering so error reporting is deterministic.
            errors.sort_by(|a, b| a.key.cmp(&b.key));
            Err(ConfigError::Validation(errors))
        }
    }
}

/// Coerce a raw YAML value to the requested [`FieldType`], or `None` on a type
/// mismatch. Integers widen to floats; everything else is strict.
fn coerce(ty: FieldType, raw: &serde_yaml::Value) -> Option<serde_yaml::Value> {
    match ty {
        FieldType::Str => raw.as_str().map(serde_yaml::Value::from),
        FieldType::Int => raw.as_i64().map(serde_yaml::Value::from),
        FieldType::Float => raw.as_f64().map(serde_yaml::Value::from),
        FieldType::Bool => raw.as_bool().map(serde_yaml::Value::from),
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
        let value: serde_yaml::Value =
            serde_yaml::from_str(yaml).map_err(|e| ConfigError::Yaml(e.to_string()))?;

        let defaults = Self::default();
        let schema = Schema::new(vec![
            Field::optional("name", FieldType::Str)
                .with_default(serde_yaml::Value::from(defaults.name.clone())),
            Field::optional("latitude", FieldType::Float)
                .with_default(serde_yaml::Value::from(defaults.latitude)),
            Field::optional("longitude", FieldType::Float)
                .with_default(serde_yaml::Value::from(defaults.longitude)),
            Field::optional("elevation", FieldType::Int)
                .with_default(serde_yaml::Value::from(defaults.elevation)),
            Field::optional("unit_system", FieldType::Str)
                .with_default(serde_yaml::Value::from("metric")),
            Field::optional("time_zone", FieldType::Str)
                .with_default(serde_yaml::Value::from(defaults.time_zone.clone())),
            Field::optional("currency", FieldType::Str)
                .with_default(serde_yaml::Value::from(defaults.currency.clone())),
            Field::optional("country", FieldType::Str),
            Field::optional("language", FieldType::Str)
                .with_default(serde_yaml::Value::from(defaults.language.clone())),
        ])
        // Core config carries integration keys we do not model; let them pass.
        .allow_extra();

        let map = schema.validate(&value)?;

        // Post-schema value checks HA enforces beyond the type schema.
        let mut errors = Vec::new();
        let str_of = |k: &str| map.get(k).and_then(serde_yaml::Value::as_str).map(str::to_owned);
        let f64_of = |k: &str| map.get(k).and_then(serde_yaml::Value::as_f64);

        let latitude = f64_of("latitude").unwrap_or(defaults.latitude);
        if !(-90.0..=90.0).contains(&latitude) {
            errors.push(ValidationError::new("latitude", "must be between -90 and 90"));
        }
        let longitude = f64_of("longitude").unwrap_or(defaults.longitude);
        if !(-180.0..=180.0).contains(&longitude) {
            errors.push(ValidationError::new("longitude", "must be between -180 and 180"));
        }

        let unit_token = str_of("unit_system").unwrap_or_else(|| "metric".to_owned());
        let unit_system = match unit_token.as_str() {
            "metric" => Some(UnitSystem::Metric),
            // HA renamed `imperial` → `us_customary`; accept both.
            "us_customary" | "imperial" => Some(UnitSystem::UsCustomary),
            _ => {
                errors.push(ValidationError::new(
                    "unit_system",
                    "must be one of: metric, us_customary",
                ));
                None
            }
        };

        if !errors.is_empty() {
            return Err(ConfigError::Validation(errors));
        }

        Ok(Self {
            name: str_of("name").unwrap_or(defaults.name),
            latitude,
            longitude,
            elevation: map.get("elevation").and_then(serde_yaml::Value::as_i64).unwrap_or(defaults.elevation),
            unit_system: unit_system.unwrap_or_default(),
            time_zone: str_of("time_zone").unwrap_or(defaults.time_zone),
            currency: str_of("currency").unwrap_or(defaults.currency),
            country: str_of("country"),
            language: str_of("language").unwrap_or(defaults.language),
        })
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
