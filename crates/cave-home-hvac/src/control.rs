//! The hysteresis control engine — decide what the device should *do* now.
//!
//! Given the current room temperature, the requested [`HvacMode`], the
//! [`Setpoint`], and a cold/hot tolerance, [`decide`] returns the
//! [`HvacAction`] the device should take. The logic follows the Home Assistant
//! *generic thermostat* hysteresis semantics precisely (public HA docs,
//! Apache-2.0; no source ported):
//!
//! - **Heat**: start heating when the room drops to `target − cold_tolerance`;
//!   stop (go [`HvacAction::Idle`]) once it reaches `target + hot_tolerance`.
//! - **Cool**: start cooling when the room rises to `target + hot_tolerance`;
//!   stop once it falls to `target − cold_tolerance`.
//! - **`HeatCool`**: heat below `low − cold_tolerance`, cool above
//!   `high + hot_tolerance`, idle inside the band.
//! - **Dry**: dehumidify above `target + hot_tolerance`.
//! - **`FanOnly`**: always [`HvacAction::Fan`] while on.
//! - **Auto / Off**: Auto behaves like a single-target heat/cool around the
//!   target; Off is always [`HvacAction::Off`].
//!
//! The hysteresis band (the gap between the start and stop thresholds) is what
//! stops the relay "chattering" on and off around the setpoint — exactly the
//! behaviour a household expects from a real thermostat.

use crate::mode::{HvacAction, HvacMode};
use crate::setpoint::Setpoint;
use crate::temperature::Temperature;

/// The symmetric-or-asymmetric dead-band around a setpoint.
///
/// `cold_tolerance` is how far *below* a heat target the room may fall before
/// heating kicks in; `hot_tolerance` is how far *above* a cool target it may
/// rise before cooling kicks in. HA's generic thermostat exposes exactly these
/// two knobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    cold: f64,
    hot: f64,
}

/// Why a [`Tolerance`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToleranceError {
    /// A tolerance was non-finite or negative.
    Invalid,
}

impl core::fmt::Display for ToleranceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("tolerance values must be finite and non-negative")
    }
}

impl std::error::Error for ToleranceError {}

impl Tolerance {
    /// Build a tolerance from a cold and a hot half-band (°C).
    ///
    /// # Errors
    /// Returns [`ToleranceError::Invalid`] if either value is non-finite or
    /// negative.
    pub fn new(cold: f64, hot: f64) -> Result<Self, ToleranceError> {
        if !cold.is_finite() || !hot.is_finite() || cold < 0.0 || hot < 0.0 {
            return Err(ToleranceError::Invalid);
        }
        Ok(Self { cold, hot })
    }

    /// A symmetric tolerance (same cold and hot half-band).
    ///
    /// # Errors
    /// Returns [`ToleranceError::Invalid`] if `value` is non-finite or negative.
    pub fn symmetric(value: f64) -> Result<Self, ToleranceError> {
        Self::new(value, value)
    }

    #[must_use]
    pub const fn cold(self) -> f64 {
        self.cold
    }

    #[must_use]
    pub const fn hot(self) -> f64 {
        self.hot
    }
}

/// The current activity to drive a single-target heat decision around `target`.
fn heat_decision(current: f64, target: f64, tol: Tolerance) -> HvacAction {
    if current <= target - tol.cold {
        HvacAction::Heating
    } else if current >= target + tol.hot {
        HvacAction::Idle
    } else {
        // Inside the dead-band: hold whatever we were doing. With no prior
        // state we report Idle — the conservative "do nothing extra" choice.
        HvacAction::Idle
    }
}

/// The current activity to drive a single-target cool decision around `target`.
fn cool_decision(current: f64, target: f64, tol: Tolerance) -> HvacAction {
    if current >= target + tol.hot {
        HvacAction::Cooling
    } else {
        HvacAction::Idle
    }
}

/// Decide the [`HvacAction`] for the current conditions.
///
/// The `setpoint` must match the `mode`'s expected shape (single vs. band); a
/// mismatch is reported as [`DecideError::SetpointShapeMismatch`] rather than
/// silently guessing.
///
/// # Errors
/// Returns [`DecideError::SetpointShapeMismatch`] if the setpoint shape does not
/// match the mode (e.g. a band given to Heat, or a single target given to
/// `HeatCool`).
pub fn decide(
    mode: HvacMode,
    current: Temperature,
    setpoint: &Setpoint,
    tol: Tolerance,
) -> Result<HvacAction, DecideError> {
    let now = current.celsius();
    match mode {
        HvacMode::Off => Ok(HvacAction::Off),
        HvacMode::FanOnly => Ok(HvacAction::Fan),
        HvacMode::Heat => {
            let target = single_target(mode, setpoint)?;
            Ok(heat_decision(now, target, tol))
        }
        HvacMode::Cool => {
            let target = single_target(mode, setpoint)?;
            Ok(cool_decision(now, target, tol))
        }
        HvacMode::Dry => {
            // Dehumidify like a cool decision: act when warm/humid above target.
            let target = single_target(mode, setpoint)?;
            if now >= target + tol.hot {
                Ok(HvacAction::Drying)
            } else {
                Ok(HvacAction::Idle)
            }
        }
        HvacMode::Auto => {
            // Auto around a single target: heat if cold, cool if hot, else idle.
            let target = single_target(mode, setpoint)?;
            if now <= target - tol.cold {
                Ok(HvacAction::Heating)
            } else if now >= target + tol.hot {
                Ok(HvacAction::Cooling)
            } else {
                Ok(HvacAction::Idle)
            }
        }
        HvacMode::HeatCool => {
            let (low, high) = band_targets(mode, setpoint)?;
            if now <= low - tol.cold {
                Ok(HvacAction::Heating)
            } else if now >= high + tol.hot {
                Ok(HvacAction::Cooling)
            } else {
                Ok(HvacAction::Idle)
            }
        }
    }
}

/// Why a [`decide`] call could not produce an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecideError {
    /// The setpoint shape (single vs. band) did not match the mode.
    SetpointShapeMismatch,
}

impl core::fmt::Display for DecideError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("the setpoint does not match the requested mode")
    }
}

impl std::error::Error for DecideError {}

fn single_target(mode: HvacMode, setpoint: &Setpoint) -> Result<f64, DecideError> {
    match setpoint {
        Setpoint::Single(t) if mode.uses_single_target() => Ok(t.celsius()),
        _ => Err(DecideError::SetpointShapeMismatch),
    }
}

fn band_targets(mode: HvacMode, setpoint: &Setpoint) -> Result<(f64, f64), DecideError> {
    match setpoint {
        Setpoint::Band { low, high } if mode.uses_target_band() => {
            Ok((low.celsius(), high.celsius()))
        }
        _ => Err(DecideError::SetpointShapeMismatch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(v: f64) -> Temperature {
        Temperature::from_celsius(v).expect("valid test temperature")
    }

    fn tol() -> Tolerance {
        Tolerance::symmetric(0.5).expect("valid tolerance")
    }

    #[test]
    fn tolerance_rejects_invalid() {
        assert_eq!(Tolerance::new(-0.1, 0.5), Err(ToleranceError::Invalid));
        assert_eq!(Tolerance::new(0.5, f64::NAN), Err(ToleranceError::Invalid));
        assert!(Tolerance::symmetric(0.0).is_ok());
    }

    #[test]
    fn off_is_always_off() {
        let sp = Setpoint::single(c(21.0));
        assert_eq!(
            decide(HvacMode::Off, c(5.0), &sp, tol()),
            Ok(HvacAction::Off)
        );
    }

    #[test]
    fn fan_only_is_always_fan() {
        let sp = Setpoint::single(c(21.0));
        assert_eq!(
            decide(HvacMode::FanOnly, c(30.0), &sp, tol()),
            Ok(HvacAction::Fan)
        );
    }

    #[test]
    fn heat_starts_at_lower_threshold() {
        let sp = Setpoint::single(c(21.0));
        // target 21, cold tol 0.5 -> heat at or below 20.5.
        assert_eq!(decide(HvacMode::Heat, c(20.4), &sp, tol()), Ok(HvacAction::Heating));
        assert_eq!(decide(HvacMode::Heat, c(20.5), &sp, tol()), Ok(HvacAction::Heating));
        // Just inside the dead-band -> idle (hold).
        assert_eq!(decide(HvacMode::Heat, c(20.6), &sp, tol()), Ok(HvacAction::Idle));
    }

    #[test]
    fn heat_stops_at_upper_threshold() {
        let sp = Setpoint::single(c(21.0));
        // hot tol 0.5 -> stop at or above 21.5.
        assert_eq!(decide(HvacMode::Heat, c(21.5), &sp, tol()), Ok(HvacAction::Idle));
        assert_eq!(decide(HvacMode::Heat, c(22.0), &sp, tol()), Ok(HvacAction::Idle));
    }

    #[test]
    fn cool_starts_at_upper_threshold() {
        let sp = Setpoint::single(c(24.0));
        // target 24, hot tol 0.5 -> cool at or above 24.5.
        assert_eq!(decide(HvacMode::Cool, c(24.6), &sp, tol()), Ok(HvacAction::Cooling));
        assert_eq!(decide(HvacMode::Cool, c(24.5), &sp, tol()), Ok(HvacAction::Cooling));
        // Below the threshold -> idle.
        assert_eq!(decide(HvacMode::Cool, c(24.4), &sp, tol()), Ok(HvacAction::Idle));
    }

    #[test]
    fn dry_acts_above_target() {
        let sp = Setpoint::single(c(22.0));
        assert_eq!(decide(HvacMode::Dry, c(22.6), &sp, tol()), Ok(HvacAction::Drying));
        assert_eq!(decide(HvacMode::Dry, c(22.0), &sp, tol()), Ok(HvacAction::Idle));
    }

    #[test]
    fn auto_heats_when_cold_cools_when_hot() {
        let sp = Setpoint::single(c(21.0));
        assert_eq!(decide(HvacMode::Auto, c(20.0), &sp, tol()), Ok(HvacAction::Heating));
        assert_eq!(decide(HvacMode::Auto, c(22.0), &sp, tol()), Ok(HvacAction::Cooling));
        assert_eq!(decide(HvacMode::Auto, c(21.0), &sp, tol()), Ok(HvacAction::Idle));
    }

    #[test]
    fn heatcool_band_heats_below_cools_above_idles_inside() {
        let sp = Setpoint::band(c(19.0), c(24.0)).expect("ordered band");
        // low 19, cold tol 0.5 -> heat at or below 18.5.
        assert_eq!(decide(HvacMode::HeatCool, c(18.4), &sp, tol()), Ok(HvacAction::Heating));
        // high 24, hot tol 0.5 -> cool at or above 24.5.
        assert_eq!(decide(HvacMode::HeatCool, c(24.6), &sp, tol()), Ok(HvacAction::Cooling));
        // Comfortable middle -> idle.
        assert_eq!(decide(HvacMode::HeatCool, c(21.0), &sp, tol()), Ok(HvacAction::Idle));
    }

    #[test]
    fn heatcool_boundaries_are_inclusive() {
        let sp = Setpoint::band(c(19.0), c(24.0)).expect("ordered band");
        assert_eq!(decide(HvacMode::HeatCool, c(18.5), &sp, tol()), Ok(HvacAction::Heating));
        assert_eq!(decide(HvacMode::HeatCool, c(24.5), &sp, tol()), Ok(HvacAction::Cooling));
        // Just inside both thresholds -> idle.
        assert_eq!(decide(HvacMode::HeatCool, c(18.6), &sp, tol()), Ok(HvacAction::Idle));
        assert_eq!(decide(HvacMode::HeatCool, c(24.4), &sp, tol()), Ok(HvacAction::Idle));
    }

    #[test]
    fn asymmetric_tolerance_is_respected() {
        let sp = Setpoint::single(c(21.0));
        let asym = Tolerance::new(1.0, 0.2).expect("valid");
        // cold tol 1.0 -> heat at or below 20.0.
        assert_eq!(decide(HvacMode::Heat, c(20.0), &sp, asym), Ok(HvacAction::Heating));
        assert_eq!(decide(HvacMode::Heat, c(20.5), &sp, asym), Ok(HvacAction::Idle));
    }

    #[test]
    fn shape_mismatch_is_rejected() {
        let band = Setpoint::band(c(19.0), c(24.0)).expect("ordered band");
        assert_eq!(
            decide(HvacMode::Heat, c(20.0), &band, tol()),
            Err(DecideError::SetpointShapeMismatch)
        );
        let single = Setpoint::single(c(21.0));
        assert_eq!(
            decide(HvacMode::HeatCool, c(20.0), &single, tol()),
            Err(DecideError::SetpointShapeMismatch)
        );
    }
}
