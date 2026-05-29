//! Charge plan — will the sun get your car ready in time?
//!
//! Given where the battery is now (`current_soc`), where it needs to be
//! (`target_soc`), how big the battery is (`capacity_kwh`), how much energy a
//! sunny session can realistically deliver before the deadline, and how many
//! hours remain, the planner reports whether spare sunshine alone will make the
//! deadline — or whether cave-home must top up from the grid to get there.
//!
//! No clock is read here: the caller supplies the **hours remaining** and an
//! estimate of the **PV power** (watts) it expects to be able to give the car
//! over that window.

use crate::error::EvccError;

/// The verdict of a charge plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanOutcome {
    /// The car is already at or above the target — nothing to do.
    AlreadyMet,
    /// Spare sunshine alone will reach the target before the deadline.
    SunWillMakeIt,
    /// The sun won't be enough in time; the grid must top up to make the
    /// deadline.
    NeedsGridTopUp,
    /// Even pulling flat-out from the grid for the whole window can't reach the
    /// target in time (the deadline is simply too tight for the battery).
    Unreachable,
}

/// The energy arithmetic behind a [`PlanOutcome`], in kWh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChargePlan {
    /// The verdict.
    pub outcome: PlanOutcome,
    /// Energy still needed to reach the target (kWh); `0.0` if already met.
    pub energy_needed_kwh: f64,
    /// Energy spare sunshine can deliver before the deadline (kWh).
    pub solar_available_kwh: f64,
    /// Shortfall the grid must cover (kWh); `0.0` if the sun covers it.
    pub grid_topup_kwh: f64,
}

/// Inputs to the charge planner.
#[derive(Debug, Clone, Copy)]
pub struct PlanInputs {
    /// Battery size, kWh (must be > 0).
    pub capacity_kwh: f64,
    /// Where the battery is now, percent (0..=100).
    pub current_soc: f64,
    /// Where the battery needs to be, percent (0..=100).
    pub target_soc: f64,
    /// Hours left before the car is needed (≥ 0).
    pub hours_remaining: f64,
    /// Spare PV power expected over the window, watts (≥ 0).
    pub expected_pv_watts: f64,
    /// The charger's maximum delivery, watts (≥ 0) — used to test whether even
    /// grid top-up could physically reach the target in the time left.
    pub max_charge_watts: f64,
}

/// Compute the charge plan.
///
/// # Errors
/// - [`EvccError::NonPositiveCapacity`] if `capacity_kwh` ≤ 0.
/// - [`EvccError::SocOutOfRange`] if either state of charge is outside `0..=100`.
/// - [`EvccError::NegativeDeadline`] if `hours_remaining` < 0.
/// - [`EvccError::NegativePower`] if a watt input is negative.
/// - [`EvccError::NotFinite`] for any non-finite input.
pub fn plan(inputs: PlanInputs) -> Result<ChargePlan, EvccError> {
    let PlanInputs {
        capacity_kwh,
        current_soc,
        target_soc,
        hours_remaining,
        expected_pv_watts,
        max_charge_watts,
    } = inputs;

    for v in [
        capacity_kwh,
        current_soc,
        target_soc,
        hours_remaining,
        expected_pv_watts,
        max_charge_watts,
    ] {
        if !v.is_finite() {
            return Err(EvccError::NotFinite);
        }
    }
    if capacity_kwh <= 0.0 {
        return Err(EvccError::NonPositiveCapacity);
    }
    if !(0.0..=100.0).contains(&current_soc) || !(0.0..=100.0).contains(&target_soc) {
        return Err(EvccError::SocOutOfRange);
    }
    if hours_remaining < 0.0 {
        return Err(EvccError::NegativeDeadline);
    }
    if expected_pv_watts < 0.0 || max_charge_watts < 0.0 {
        return Err(EvccError::NegativePower);
    }

    // Energy needed to lift the battery from current to target SoC.
    let deficit_fraction = ((target_soc - current_soc) / 100.0).max(0.0);
    let energy_needed_kwh = deficit_fraction * capacity_kwh;

    if energy_needed_kwh <= 0.0 {
        return Ok(ChargePlan {
            outcome: PlanOutcome::AlreadyMet,
            energy_needed_kwh: 0.0,
            solar_available_kwh: 0.0,
            grid_topup_kwh: 0.0,
        });
    }

    // Energy each source can deliver in the time left (watts -> kWh).
    let solar_available_kwh = expected_pv_watts / 1000.0 * hours_remaining;
    let max_deliverable_kwh = max_charge_watts / 1000.0 * hours_remaining;

    if solar_available_kwh >= energy_needed_kwh {
        return Ok(ChargePlan {
            outcome: PlanOutcome::SunWillMakeIt,
            energy_needed_kwh,
            solar_available_kwh,
            grid_topup_kwh: 0.0,
        });
    }

    // The sun falls short. Can the charger (sun + grid) physically reach the
    // target in the window at all?
    let grid_topup_kwh = energy_needed_kwh - solar_available_kwh;
    let outcome = if max_deliverable_kwh >= energy_needed_kwh {
        PlanOutcome::NeedsGridTopUp
    } else {
        PlanOutcome::Unreachable
    };

    Ok(ChargePlan { outcome, energy_needed_kwh, solar_available_kwh, grid_topup_kwh })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PlanInputs {
        PlanInputs {
            capacity_kwh: 60.0,
            current_soc: 50.0,
            target_soc: 80.0,
            hours_remaining: 8.0,
            expected_pv_watts: 3000.0,
            max_charge_watts: 11_000.0,
        }
    }

    #[test]
    fn already_met_when_at_or_above_target() {
        let mut i = base();
        i.current_soc = 80.0;
        let p = plan(i).unwrap();
        assert_eq!(p.outcome, PlanOutcome::AlreadyMet);
        assert_eq!(p.energy_needed_kwh, 0.0);

        i.current_soc = 90.0;
        assert_eq!(plan(i).unwrap().outcome, PlanOutcome::AlreadyMet);
    }

    #[test]
    fn sun_makes_it_with_a_long_sunny_window() {
        // Need 30% of 60 kWh = 18 kWh; 3 kW * 8 h = 24 kWh of sun -> sun wins.
        let p = plan(base()).unwrap();
        assert_eq!(p.outcome, PlanOutcome::SunWillMakeIt);
        assert!((p.energy_needed_kwh - 18.0).abs() < 1e-9);
        assert_eq!(p.grid_topup_kwh, 0.0);
    }

    #[test]
    fn needs_grid_topup_when_sun_falls_short_but_charger_can_reach() {
        let mut i = base();
        i.hours_remaining = 2.0; // 3 kW * 2 h = 6 kWh sun, need 18 kWh.
        let p = plan(i).unwrap();
        assert_eq!(p.outcome, PlanOutcome::NeedsGridTopUp);
        assert!((p.grid_topup_kwh - 12.0).abs() < 1e-9); // 18 - 6
    }

    #[test]
    fn unreachable_when_even_full_charger_cannot_make_deadline() {
        let mut i = base();
        i.hours_remaining = 1.0;
        i.expected_pv_watts = 0.0;
        i.max_charge_watts = 11_000.0; // 11 kWh max in 1 h, need 18 kWh.
        let p = plan(i).unwrap();
        assert_eq!(p.outcome, PlanOutcome::Unreachable);
    }

    #[test]
    fn boundary_sun_exactly_meets_need() {
        let mut i = base();
        // Need 18 kWh; make sun deliver exactly 18 kWh: 2250 W * 8 h.
        i.expected_pv_watts = 2250.0;
        let p = plan(i).unwrap();
        assert_eq!(p.outcome, PlanOutcome::SunWillMakeIt);
    }

    #[test]
    fn zero_hours_remaining_is_unreachable_when_energy_needed() {
        let mut i = base();
        i.hours_remaining = 0.0;
        assert_eq!(plan(i).unwrap().outcome, PlanOutcome::Unreachable);
    }

    #[test]
    fn rejects_bad_capacity_soc_and_deadline() {
        let mut i = base();
        i.capacity_kwh = 0.0;
        assert_eq!(plan(i), Err(EvccError::NonPositiveCapacity));

        let mut i = base();
        i.current_soc = 120.0;
        assert_eq!(plan(i), Err(EvccError::SocOutOfRange));

        let mut i = base();
        i.target_soc = -1.0;
        assert_eq!(plan(i), Err(EvccError::SocOutOfRange));

        let mut i = base();
        i.hours_remaining = -1.0;
        assert_eq!(plan(i), Err(EvccError::NegativeDeadline));

        let mut i = base();
        i.expected_pv_watts = -1.0;
        assert_eq!(plan(i), Err(EvccError::NegativePower));

        let mut i = base();
        i.capacity_kwh = f64::NAN;
        assert_eq!(plan(i), Err(EvccError::NotFinite));
    }
}
