//! Phase switching — one phase or three?
//!
//! A three-phase charger can push three times the power of a single phase at
//! the same current, but it also has a *higher* minimum: it cannot run below
//! `3 * min_amps * voltage` watts. On a sunny day that headroom is great; on a
//! thin day three phases would never reach their minimum and the car would
//! sit idle, where a single phase could happily trickle.
//!
//! So cave-home steps **up** to three phases only when the surplus comfortably
//! supports the three-phase minimum, and steps **down** to one phase when it
//! drops below the single-phase ceiling. A hysteresis margin between the two
//! thresholds stops the system from flapping back and forth on every passing
//! cloud.

use crate::error::EvccError;

/// How many phases the car is charging on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseCount {
    Single,
    Three,
}

impl PhaseCount {
    /// The wire count (1 or 3) for the current-sizing math.
    #[must_use]
    pub const fn count(self) -> u8 {
        match self {
            Self::Single => 1,
            Self::Three => 3,
        }
    }
}

/// Decide the phase count for the next setpoint, given the current phase, the
/// available surplus, and the charger's electrical parameters.
///
/// - `surplus_w`   — spare watts available to the car.
/// - `min_amps`    — the charger's per-phase minimum current.
/// - `voltage`     — grid voltage.
/// - `current`     — the phase count in use right now.
/// - `margin_w`    — hysteresis band (watts) added on top of the up-switch
///   threshold; widening it makes switching lazier and less flappy.
///
/// Thresholds:
/// - up-switch (1→3) when `surplus ≥ 3 * min_amps * voltage + margin`.
/// - down-switch (3→1) when `surplus < 3 * min_amps * voltage` (the bare
///   three-phase minimum — below it three phases can't sustain charging).
///
/// The gap between the two is the hysteresis band: inside it, the current
/// phase count is held.
///
/// # Errors
/// [`EvccError::NotFinite`] for non-finite inputs,
/// [`EvccError::NegativeCurrent`] / [`EvccError::NegativePower`] for negative
/// `min_amps` / `margin_w`, and [`EvccError::NonPositiveVoltage`] if voltage
/// ≤ 0.
pub fn decide_phases(
    surplus_w: f64,
    min_amps: f64,
    voltage: f64,
    current: PhaseCount,
    margin_w: f64,
) -> Result<PhaseCount, EvccError> {
    if !surplus_w.is_finite() || !min_amps.is_finite() || !voltage.is_finite()
        || !margin_w.is_finite()
    {
        return Err(EvccError::NotFinite);
    }
    if voltage <= 0.0 {
        return Err(EvccError::NonPositiveVoltage);
    }
    if min_amps < 0.0 {
        return Err(EvccError::NegativeCurrent);
    }
    if margin_w < 0.0 {
        return Err(EvccError::NegativePower);
    }

    let three_phase_min = 3.0 * min_amps * voltage;
    let up_threshold = three_phase_min + margin_w;

    Ok(match current {
        PhaseCount::Single => {
            if surplus_w >= up_threshold {
                PhaseCount::Three
            } else {
                PhaseCount::Single
            }
        }
        PhaseCount::Three => {
            if surplus_w < three_phase_min {
                PhaseCount::Single
            } else {
                PhaseCount::Three
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: f64 = 230.0;
    const MIN: f64 = 6.0;
    // 3 * 6 A * 230 V = 4140 W three-phase minimum.
    const TP_MIN: f64 = 3.0 * MIN * V;
    const MARGIN: f64 = 500.0;

    #[test]
    fn phase_count_wire_counts() {
        assert_eq!(PhaseCount::Single.count(), 1);
        assert_eq!(PhaseCount::Three.count(), 3);
    }

    #[test]
    fn steps_up_when_surplus_clears_threshold_plus_margin() {
        let p = decide_phases(TP_MIN + MARGIN, MIN, V, PhaseCount::Single, MARGIN).unwrap();
        assert_eq!(p, PhaseCount::Three);
    }

    #[test]
    fn holds_single_just_below_up_threshold() {
        let p = decide_phases(TP_MIN + MARGIN - 1.0, MIN, V, PhaseCount::Single, MARGIN).unwrap();
        assert_eq!(p, PhaseCount::Single);
    }

    #[test]
    fn steps_down_when_surplus_falls_below_bare_minimum() {
        let p = decide_phases(TP_MIN - 1.0, MIN, V, PhaseCount::Three, MARGIN).unwrap();
        assert_eq!(p, PhaseCount::Single);
    }

    #[test]
    fn holds_three_at_bare_minimum() {
        let p = decide_phases(TP_MIN, MIN, V, PhaseCount::Three, MARGIN).unwrap();
        assert_eq!(p, PhaseCount::Three);
    }

    #[test]
    fn hysteresis_band_holds_current_phase_either_way() {
        // Inside the band [TP_MIN, TP_MIN+MARGIN): one-phase stays one,
        // three-phase stays three — no flapping.
        let mid = TP_MIN + MARGIN / 2.0;
        assert_eq!(
            decide_phases(mid, MIN, V, PhaseCount::Single, MARGIN).unwrap(),
            PhaseCount::Single
        );
        assert_eq!(
            decide_phases(mid, MIN, V, PhaseCount::Three, MARGIN).unwrap(),
            PhaseCount::Three
        );
    }

    #[test]
    fn rejects_bad_inputs() {
        assert_eq!(
            decide_phases(1000.0, MIN, 0.0, PhaseCount::Single, MARGIN),
            Err(EvccError::NonPositiveVoltage)
        );
        assert_eq!(
            decide_phases(1000.0, -1.0, V, PhaseCount::Single, MARGIN),
            Err(EvccError::NegativeCurrent)
        );
        assert_eq!(
            decide_phases(1000.0, MIN, V, PhaseCount::Single, -1.0),
            Err(EvccError::NegativePower)
        );
        assert_eq!(
            decide_phases(f64::NAN, MIN, V, PhaseCount::Single, MARGIN),
            Err(EvccError::NotFinite)
        );
    }
}
