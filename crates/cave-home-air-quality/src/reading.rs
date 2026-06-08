//! Sensor reading model — the typed inputs the air-quality engine consumes.
//!
//! Readings are vendor-neutral: an AirGradient, Awair, IKEA Vindriktning or
//! Airthings adapter (all deferred to phase-1b, see the parity manifest) maps
//! its wire format onto these types, and everything downstream — AQI, category,
//! room assessment — works off this model alone.

/// A single pollutant or environmental quantity cave-home can reason about.
///
/// Concentration-unit conventions follow the US EPA AQI reference tables so the
/// [`crate::aqi`] engine can index breakpoints directly:
/// - `Pm25`, `Pm10`  — micrograms per cubic metre (µg/m³)
/// - `Ozone`, `CarbonMonoxide` — parts per million (ppm)
/// - `NitrogenDioxide`, `SulfurDioxide` — parts per billion (ppb)
/// - `CarbonDioxide` — parts per million (ppm)
/// - `VocIndex` — Sensirion VOC Index points (1..=500, 100 ≈ typical)
/// - `Radon` — becquerels per cubic metre (Bq/m³)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pollutant {
    Pm25,
    Pm10,
    Ozone,
    NitrogenDioxide,
    SulfurDioxide,
    CarbonMonoxide,
    CarbonDioxide,
    VocIndex,
    Radon,
}

impl Pollutant {
    /// Short, end-user-facing label (Charter §6.3 grandma-friendly UX — no
    /// chemistry jargon beyond the household-familiar shorthand).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pm25 => "Fine dust (PM2.5)",
            Self::Pm10 => "Coarse dust (PM10)",
            Self::Ozone => "Ozone",
            Self::NitrogenDioxide => "Nitrogen dioxide",
            Self::SulfurDioxide => "Sulfur dioxide",
            Self::CarbonMonoxide => "Carbon monoxide",
            Self::CarbonDioxide => "CO₂ (stuffiness)",
            Self::VocIndex => "Chemical smells (VOC)",
            Self::Radon => "Radon",
        }
    }

    /// The unit string the value is expressed in.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Pm25 | Self::Pm10 => "µg/m³",
            Self::Ozone | Self::CarbonMonoxide | Self::CarbonDioxide => "ppm",
            Self::NitrogenDioxide | Self::SulfurDioxide => "ppb",
            Self::VocIndex => "index",
            Self::Radon => "Bq/m³",
        }
    }

    /// Whether this pollutant participates in the US EPA AQI ([`crate::aqi`]).
    /// CO₂, the VOC index and radon are graded by their own classifiers
    /// ([`crate::classify`]) rather than the AQI breakpoint tables.
    #[must_use]
    pub const fn has_epa_aqi(self) -> bool {
        matches!(
            self,
            Self::Pm25
                | Self::Pm10
                | Self::Ozone
                | Self::NitrogenDioxide
                | Self::SulfurDioxide
                | Self::CarbonMonoxide
        )
    }
}

/// One measurement: a pollutant and its current value.
///
/// Construction rejects non-finite or negative values up front so the rest of
/// the engine never has to defend against `NaN`/negative concentrations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pollutant: Pollutant,
    value: f64,
}

/// Why a [`Reading`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingError {
    /// The value was `NaN` or infinite.
    NotFinite,
    /// The value was below zero — physically impossible for a concentration.
    Negative,
}

impl core::fmt::Display for ReadingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFinite => f.write_str("measurement value is not finite"),
            Self::Negative => f.write_str("measurement value is negative"),
        }
    }
}

impl std::error::Error for ReadingError {}

impl Reading {
    /// Construct a validated reading.
    ///
    /// # Errors
    /// Returns [`ReadingError`] if `value` is non-finite or negative.
    pub fn new(pollutant: Pollutant, value: f64) -> Result<Self, ReadingError> {
        if !value.is_finite() {
            return Err(ReadingError::NotFinite);
        }
        if value < 0.0 {
            return Err(ReadingError::Negative);
        }
        Ok(Self { pollutant, value })
    }

    #[must_use]
    pub const fn pollutant(&self) -> Pollutant {
        self.pollutant
    }

    #[must_use]
    pub const fn value(&self) -> f64 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nan_and_infinite() {
        assert_eq!(
            Reading::new(Pollutant::Pm25, f64::NAN),
            Err(ReadingError::NotFinite)
        );
        assert_eq!(
            Reading::new(Pollutant::Pm25, f64::INFINITY),
            Err(ReadingError::NotFinite)
        );
    }

    #[test]
    fn rejects_negative() {
        assert_eq!(
            Reading::new(Pollutant::Ozone, -0.001),
            Err(ReadingError::Negative)
        );
    }

    #[test]
    fn accepts_zero_and_positive() {
        assert!(Reading::new(Pollutant::Pm25, 0.0).is_ok());
        assert_eq!(Reading::new(Pollutant::Pm25, 12.3).unwrap().value(), 12.3);
    }

    #[test]
    fn epa_aqi_membership_matches_classifier_split() {
        assert!(Pollutant::Pm25.has_epa_aqi());
        assert!(Pollutant::CarbonMonoxide.has_epa_aqi());
        assert!(!Pollutant::CarbonDioxide.has_epa_aqi());
        assert!(!Pollutant::VocIndex.has_epa_aqi());
        assert!(!Pollutant::Radon.has_epa_aqi());
    }
}
