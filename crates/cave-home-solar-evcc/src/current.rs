//! Charge-current sizing — turning spare watts into an amperage for the car.
//!
//! A charger draws roughly `current * voltage * phases` watts. Inverting that:
//! given a surplus of watts, a phase count and the grid voltage, the target
//! current is `surplus / (voltage * phases)`, clamped to the charger's
//! electrical limits (typically 6 A at the bottom — below which most EVs
//! refuse to charge — and 16 A or 32 A at the top).

use crate::error::EvccError;
use crate::mode::ChargeMode;

/// A charger's electrical current window, in amperes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurrentLimits {
    min_a: f64,
    max_a: f64,
}

impl CurrentLimits {
    /// Build a validated current window.
    ///
    /// # Errors
    /// [`EvccError::NotFinite`] for non-finite bounds,
    /// [`EvccError::NegativeCurrent`] for a negative bound, and
    /// [`EvccError::CurrentRangeInverted`] if `min_a > max_a`.
    pub fn new(min_a: f64, max_a: f64) -> Result<Self, EvccError> {
        if !min_a.is_finite() || !max_a.is_finite() {
            return Err(EvccError::NotFinite);
        }
        if min_a < 0.0 || max_a < 0.0 {
            return Err(EvccError::NegativeCurrent);
        }
        if min_a > max_a {
            return Err(EvccError::CurrentRangeInverted);
        }
        Ok(Self { min_a, max_a })
    }

    /// A common single-/three-phase home wallbox: 6 A minimum, 16 A maximum.
    #[must_use]
    pub const fn typical_16a() -> Self {
        // Bounds are constant and valid by construction.
        Self { min_a: 6.0, max_a: 16.0 }
    }

    /// A larger wallbox: 6 A minimum, 32 A maximum.
    #[must_use]
    pub const fn typical_32a() -> Self {
        Self { min_a: 6.0, max_a: 32.0 }
    }

    #[must_use]
    pub const fn min_a(&self) -> f64 {
        self.min_a
    }

    #[must_use]
    pub const fn max_a(&self) -> f64 {
        self.max_a
    }
}

/// What the engine decided to do with the car this instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChargeSetpoint {
    /// Charge the car at this current (amperes) on this phase count.
    Charge { amps: f64, phases: u8 },
    /// Pause — there isn't enough sun to reach the charger's minimum, and the
    /// mode won't pull from the grid.
    Paused,
}

impl ChargeSetpoint {
    /// The chosen current in amperes (0 while paused).
    #[must_use]
    pub const fn amps(&self) -> f64 {
        match self {
            Self::Charge { amps, .. } => *amps,
            Self::Paused => 0.0,
        }
    }

    /// Whether the car is being charged.
    #[must_use]
    pub const fn is_charging(&self) -> bool {
        matches!(self, Self::Charge { .. })
    }
}

/// The watts a given current draws across `phases` at `voltage`.
#[must_use]
pub fn power_for_current(amps: f64, voltage: f64, phases: u8) -> f64 {
    amps * voltage * f64::from(phases)
}

/// The raw (unclamped) current spare watts can support across `phases`.
///
/// # Errors
/// [`EvccError::NonPositiveVoltage`] if voltage ≤ 0,
/// [`EvccError::UnsupportedPhases`] if `phases` is not 1 or 3, and
/// [`EvccError::NotFinite`] for non-finite inputs.
pub fn current_for_surplus(surplus_w: f64, voltage: f64, phases: u8) -> Result<f64, EvccError> {
    if !surplus_w.is_finite() || !voltage.is_finite() {
        return Err(EvccError::NotFinite);
    }
    if voltage <= 0.0 {
        return Err(EvccError::NonPositiveVoltage);
    }
    if phases != 1 && phases != 3 {
        return Err(EvccError::UnsupportedPhases);
    }
    Ok(surplus_w / (voltage * f64::from(phases)))
}

/// Decide the charge setpoint for the car.
///
/// Given the spare watts, the charger's limits, the grid voltage, the phase
/// count and the [`ChargeMode`]:
/// - **`Off`** always pauses.
/// - **`Now`** charges at the charger maximum regardless of sun.
/// - **`PvOnly`** charges at the current the surplus supports, but **pauses**
///   if that is below the charger minimum (e.g. < ~1.4 kW single-phase).
/// - **`MinPlusPv`** never pauses while plugged in: it charges at *at least*
///   the minimum (drawing the shortfall from the grid) and rides any surplus
///   above that up to the maximum.
///
/// # Errors
/// Propagates the validation errors of [`current_for_surplus`].
pub fn decide_current(
    surplus_w: f64,
    limits: CurrentLimits,
    voltage: f64,
    phases: u8,
    mode: ChargeMode,
) -> Result<ChargeSetpoint, EvccError> {
    let raw = current_for_surplus(surplus_w, voltage, phases)?;

    match mode {
        ChargeMode::Off => Ok(ChargeSetpoint::Paused),
        ChargeMode::Now => Ok(ChargeSetpoint::Charge { amps: limits.max_a, phases }),
        ChargeMode::PvOnly => {
            if raw < limits.min_a {
                Ok(ChargeSetpoint::Paused)
            } else {
                Ok(ChargeSetpoint::Charge { amps: raw.min(limits.max_a), phases })
            }
        }
        ChargeMode::MinPlusPv => {
            // Guarantee the minimum (grid tops up the shortfall), ride surplus
            // above it, never exceed the maximum.
            let amps = raw.clamp(limits.min_a, limits.max_a);
            Ok(ChargeSetpoint::Charge { amps, phases })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: f64 = 230.0;

    #[test]
    fn limits_reject_inverted_range() {
        assert_eq!(
            CurrentLimits::new(16.0, 6.0),
            Err(EvccError::CurrentRangeInverted)
        );
    }

    #[test]
    fn limits_reject_negative_and_non_finite() {
        assert_eq!(CurrentLimits::new(-1.0, 16.0), Err(EvccError::NegativeCurrent));
        assert_eq!(CurrentLimits::new(6.0, f64::NAN), Err(EvccError::NotFinite));
    }

    #[test]
    fn current_for_surplus_single_phase() {
        // 3680 W / 230 V / 1 phase = 16 A.
        let a = current_for_surplus(3680.0, V, 1).unwrap();
        assert!((a - 16.0).abs() < 1e-9);
    }

    #[test]
    fn current_for_surplus_three_phase() {
        // 11040 W / 230 V / 3 phases = 16 A.
        let a = current_for_surplus(11_040.0, V, 3).unwrap();
        assert!((a - 16.0).abs() < 1e-9);
    }

    #[test]
    fn current_for_surplus_rejects_bad_phases_and_voltage() {
        assert_eq!(current_for_surplus(1000.0, V, 2), Err(EvccError::UnsupportedPhases));
        assert_eq!(current_for_surplus(1000.0, 0.0, 1), Err(EvccError::NonPositiveVoltage));
    }

    #[test]
    fn pv_only_pauses_below_single_phase_minimum() {
        // 1000 W single-phase -> ~4.35 A, below the 6 A floor -> pause.
        let s = decide_current(1000.0, CurrentLimits::typical_16a(), V, 1, ChargeMode::PvOnly)
            .unwrap();
        assert_eq!(s, ChargeSetpoint::Paused);
    }

    #[test]
    fn pv_only_charges_at_exactly_the_minimum_boundary() {
        // 6 A * 230 V = 1380 W -> exactly the minimum, should charge at 6 A.
        let s = decide_current(1380.0, CurrentLimits::typical_16a(), V, 1, ChargeMode::PvOnly)
            .unwrap();
        match s {
            ChargeSetpoint::Charge { amps, phases } => {
                assert!((amps - 6.0).abs() < 1e-9);
                assert_eq!(phases, 1);
            }
            ChargeSetpoint::Paused => panic!("should charge at the boundary"),
        }
    }

    #[test]
    fn pv_only_clamps_to_max() {
        // 10 kW single-phase would be ~43 A; clamp to 16 A.
        let s = decide_current(10_000.0, CurrentLimits::typical_16a(), V, 1, ChargeMode::PvOnly)
            .unwrap();
        assert_eq!(s.amps(), 16.0);
    }

    #[test]
    fn min_plus_pv_draws_minimum_from_grid_when_sun_is_short() {
        // Almost no sun: still charge at the 6 A minimum (grid tops up).
        let s = decide_current(200.0, CurrentLimits::typical_16a(), V, 1, ChargeMode::MinPlusPv)
            .unwrap();
        assert!(s.is_charging());
        assert!((s.amps() - 6.0).abs() < 1e-9);
    }

    #[test]
    fn min_plus_pv_rides_surplus_above_minimum() {
        // ~2300 W -> 10 A, above the minimum, below max -> charge at 10 A.
        let s = decide_current(2300.0, CurrentLimits::typical_16a(), V, 1, ChargeMode::MinPlusPv)
            .unwrap();
        assert!((s.amps() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn now_charges_at_max_regardless_of_sun() {
        let s = decide_current(0.0, CurrentLimits::typical_32a(), V, 1, ChargeMode::Now).unwrap();
        assert_eq!(s.amps(), 32.0);
    }

    #[test]
    fn off_always_pauses() {
        let s = decide_current(9999.0, CurrentLimits::typical_16a(), V, 3, ChargeMode::Off)
            .unwrap();
        assert_eq!(s, ChargeSetpoint::Paused);
    }

    #[test]
    fn power_for_current_round_trips() {
        assert!((power_for_current(16.0, V, 3) - 11_040.0).abs() < 1e-9);
    }
}
