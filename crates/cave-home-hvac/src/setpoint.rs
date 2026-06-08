//! Setpoints and device capabilities.
//!
//! A climate device is configured against a single `target_temperature`
//! ([`Setpoint::Single`]) for [`HvacMode::Heat`] / [`HvacMode::Cool`] /
//! [`HvacMode::Dry`], or a low/high comfort band ([`Setpoint::Band`]) for
//! [`HvacMode::HeatCool`]. The HA invariant `target_temp_low < target_temp_high`
//! is enforced at construction.
//!
//! [`Capabilities`] describes what a particular device can do — its supported
//! temperature range, target step, fan modes and presets — so a request for a
//! fan speed or preset the device lacks can be rejected up front (HA gates these
//! the same way through `supported_features` + the supported-mode lists).

use crate::mode::{FanMode, HvacMode, PresetMode};
use crate::temperature::Temperature;

/// A configured target for the active mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setpoint {
    /// A single target temperature (Heat / Cool / Dry).
    Single(Temperature),
    /// A comfort band, low below high (`HeatCool`).
    Band {
        /// Heat when the room falls below this.
        low: Temperature,
        /// Cool when the room rises above this.
        high: Temperature,
    },
}

/// Why a [`Setpoint`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetpointError {
    /// A band's low was not strictly below its high.
    BandNotOrdered,
}

impl core::fmt::Display for SetpointError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BandNotOrdered => {
                f.write_str("the low target must be strictly below the high target")
            }
        }
    }
}

impl std::error::Error for SetpointError {}

impl Setpoint {
    /// A single-target setpoint.
    #[must_use]
    pub const fn single(target: Temperature) -> Self {
        Self::Single(target)
    }

    /// A comfort band.
    ///
    /// # Errors
    /// Returns [`SetpointError::BandNotOrdered`] unless `low` is strictly below
    /// `high` (the HA `target_temp_low < target_temp_high` invariant).
    pub fn band(low: Temperature, high: Temperature) -> Result<Self, SetpointError> {
        if low.celsius() < high.celsius() {
            Ok(Self::Band { low, high })
        } else {
            Err(SetpointError::BandNotOrdered)
        }
    }

    /// Whether this setpoint shape matches what `mode` expects.
    #[must_use]
    pub const fn matches_mode(&self, mode: HvacMode) -> bool {
        match self {
            Self::Single(_) => mode.uses_single_target(),
            Self::Band { .. } => mode.uses_target_band(),
        }
    }
}

/// What a particular device supports.
///
/// A request for a fan mode, preset, or out-of-range/off-step temperature is
/// rejected against this, so cave-home never asks a device to do something it
/// physically cannot.
#[derive(Debug, Clone, PartialEq)]
pub struct Capabilities {
    min_temp: Temperature,
    max_temp: Temperature,
    /// Granularity of an acceptable target, in °C (e.g. 0.5 °C).
    target_step: f64,
    fan_modes: Vec<FanMode>,
    presets: Vec<PresetMode>,
}

/// Why a capability check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityError {
    /// `min_temp` was not strictly below `max_temp`, or the step was not a
    /// finite positive number.
    InvalidCapabilities,
    /// A requested temperature was outside the device's supported range.
    TemperatureOutOfRange,
    /// A requested temperature did not land on the device's target step.
    TemperatureOffStep,
    /// A requested fan mode is not supported by the device.
    UnsupportedFanMode,
    /// A requested preset is not supported by the device.
    UnsupportedPreset,
}

impl core::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidCapabilities => f.write_str("device capabilities are inconsistent"),
            Self::TemperatureOutOfRange => {
                f.write_str("requested temperature is outside the supported range")
            }
            Self::TemperatureOffStep => {
                f.write_str("requested temperature does not match the target step")
            }
            Self::UnsupportedFanMode => f.write_str("this device does not offer that fan speed"),
            Self::UnsupportedPreset => f.write_str("this device does not offer that preset"),
        }
    }
}

impl std::error::Error for CapabilityError {}

impl Capabilities {
    /// Build a capability description.
    ///
    /// # Errors
    /// Returns [`CapabilityError::InvalidCapabilities`] if `min_temp` is not
    /// strictly below `max_temp`, or `target_step` is not finite and positive.
    pub fn new(
        min_temp: Temperature,
        max_temp: Temperature,
        target_step: f64,
        fan_modes: Vec<FanMode>,
        presets: Vec<PresetMode>,
    ) -> Result<Self, CapabilityError> {
        if min_temp.celsius() >= max_temp.celsius()
            || !target_step.is_finite()
            || target_step <= 0.0
        {
            return Err(CapabilityError::InvalidCapabilities);
        }
        Ok(Self {
            min_temp,
            max_temp,
            target_step,
            fan_modes,
            presets,
        })
    }

    #[must_use]
    pub const fn min_temp(&self) -> Temperature {
        self.min_temp
    }

    #[must_use]
    pub const fn max_temp(&self) -> Temperature {
        self.max_temp
    }

    #[must_use]
    pub const fn target_step(&self) -> f64 {
        self.target_step
    }

    #[must_use]
    pub fn supports_fan_mode(&self, fan: FanMode) -> bool {
        self.fan_modes.contains(&fan)
    }

    #[must_use]
    pub fn supports_preset(&self, preset: PresetMode) -> bool {
        self.presets.contains(&preset)
    }

    /// Check a single temperature against the supported range and step.
    ///
    /// # Errors
    /// Returns [`CapabilityError::TemperatureOutOfRange`] if outside
    /// `min..=max`, or [`CapabilityError::TemperatureOffStep`] if it does not
    /// land on the target step measured from `min_temp`.
    pub fn check_temperature(&self, temp: Temperature) -> Result<(), CapabilityError> {
        let c = temp.celsius();
        if c < self.min_temp.celsius() || c > self.max_temp.celsius() {
            return Err(CapabilityError::TemperatureOutOfRange);
        }
        let offset = c - self.min_temp.celsius();
        let steps = offset / self.target_step;
        // Accept the nearest step within a tight epsilon so floating-point
        // setpoints like 20.5 on a 0.5 grid are not spuriously rejected.
        if (steps - steps.round()).abs() > 1e-6 {
            return Err(CapabilityError::TemperatureOffStep);
        }
        Ok(())
    }

    /// Validate a [`Setpoint`] against this device's range and step.
    ///
    /// # Errors
    /// Returns the first [`CapabilityError`] encountered for either endpoint.
    pub fn check_setpoint(&self, setpoint: &Setpoint) -> Result<(), CapabilityError> {
        match setpoint {
            Setpoint::Single(t) => self.check_temperature(*t),
            Setpoint::Band { low, high } => {
                self.check_temperature(*low)?;
                self.check_temperature(*high)
            }
        }
    }

    /// Validate a requested fan mode.
    ///
    /// # Errors
    /// Returns [`CapabilityError::UnsupportedFanMode`] if the device does not
    /// offer `fan`.
    pub fn check_fan_mode(&self, fan: FanMode) -> Result<(), CapabilityError> {
        if self.supports_fan_mode(fan) {
            Ok(())
        } else {
            Err(CapabilityError::UnsupportedFanMode)
        }
    }

    /// Validate a requested preset.
    ///
    /// # Errors
    /// Returns [`CapabilityError::UnsupportedPreset`] if the device does not
    /// offer `preset`.
    pub fn check_preset(&self, preset: PresetMode) -> Result<(), CapabilityError> {
        if self.supports_preset(preset) {
            Ok(())
        } else {
            Err(CapabilityError::UnsupportedPreset)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(v: f64) -> Temperature {
        Temperature::from_celsius(v).expect("valid test temperature")
    }

    fn caps() -> Capabilities {
        Capabilities::new(
            c(7.0),
            c(35.0),
            0.5,
            vec![FanMode::Auto, FanMode::Low, FanMode::High],
            vec![PresetMode::None, PresetMode::Eco, PresetMode::Away],
        )
        .expect("valid capabilities")
    }

    #[test]
    fn band_requires_low_below_high() {
        assert!(Setpoint::band(c(18.0), c(24.0)).is_ok());
        assert_eq!(
            Setpoint::band(c(24.0), c(18.0)),
            Err(SetpointError::BandNotOrdered)
        );
        // Equal is not strictly ordered.
        assert_eq!(
            Setpoint::band(c(21.0), c(21.0)),
            Err(SetpointError::BandNotOrdered)
        );
    }

    #[test]
    fn setpoint_shape_matches_mode() {
        let single = Setpoint::single(c(21.0));
        let band = Setpoint::band(c(18.0), c(24.0)).expect("ordered band");
        assert!(single.matches_mode(HvacMode::Heat));
        assert!(!single.matches_mode(HvacMode::HeatCool));
        assert!(band.matches_mode(HvacMode::HeatCool));
        assert!(!band.matches_mode(HvacMode::Heat));
    }

    #[test]
    fn invalid_capabilities_rejected() {
        assert_eq!(
            Capabilities::new(c(30.0), c(10.0), 0.5, vec![], vec![]),
            Err(CapabilityError::InvalidCapabilities)
        );
        assert_eq!(
            Capabilities::new(c(10.0), c(30.0), 0.0, vec![], vec![]),
            Err(CapabilityError::InvalidCapabilities)
        );
        assert_eq!(
            Capabilities::new(c(10.0), c(30.0), f64::NAN, vec![], vec![]),
            Err(CapabilityError::InvalidCapabilities)
        );
    }

    #[test]
    fn temperature_range_rejection() {
        let caps = caps();
        assert_eq!(
            caps.check_temperature(c(5.0)),
            Err(CapabilityError::TemperatureOutOfRange)
        );
        assert_eq!(
            caps.check_temperature(c(40.0)),
            Err(CapabilityError::TemperatureOutOfRange)
        );
        assert!(caps.check_temperature(c(7.0)).is_ok());
        assert!(caps.check_temperature(c(35.0)).is_ok());
    }

    #[test]
    fn temperature_step_rejection() {
        let caps = caps();
        assert!(caps.check_temperature(c(21.0)).is_ok());
        assert!(caps.check_temperature(c(21.5)).is_ok());
        assert_eq!(
            caps.check_temperature(c(21.3)),
            Err(CapabilityError::TemperatureOffStep)
        );
    }

    #[test]
    fn band_setpoint_validated_on_both_ends() {
        let caps = caps();
        let ok = Setpoint::band(c(18.0), c(24.0)).expect("ordered");
        assert!(caps.check_setpoint(&ok).is_ok());
        let high_too_high = Setpoint::band(c(18.0), c(40.0)).expect("ordered");
        assert_eq!(
            caps.check_setpoint(&high_too_high),
            Err(CapabilityError::TemperatureOutOfRange)
        );
    }

    #[test]
    fn fan_capability_gating() {
        let caps = caps();
        assert!(caps.check_fan_mode(FanMode::Auto).is_ok());
        assert!(caps.check_fan_mode(FanMode::Low).is_ok());
        assert_eq!(
            caps.check_fan_mode(FanMode::Medium),
            Err(CapabilityError::UnsupportedFanMode)
        );
    }

    #[test]
    fn preset_capability_gating() {
        let caps = caps();
        assert!(caps.check_preset(PresetMode::Eco).is_ok());
        assert_eq!(
            caps.check_preset(PresetMode::Boost),
            Err(CapabilityError::UnsupportedPreset)
        );
    }
}
