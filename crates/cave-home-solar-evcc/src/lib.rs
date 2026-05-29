// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-solar-evcc` — the **brain** that charges your car from the sun.
//!
//! This crate is a solar-surplus EV charge-control decision engine. It takes a
//! snapshot of the home's power flows and a charge mode, and decides whether to
//! charge the car, from where (sun or grid), at what current, on one phase or
//! three — and tells the household, in plain language, what it's doing and why.
//!
//! The charging semantics mirror the well-documented `evcc`-class behaviour
//! (modes, PV-surplus tracking, phase switching, anti-flap dwell, deadline
//! planning), but the logic here is a **first-party, clean-room implementation
//! from those documented semantics** — no upstream code was copied. The crate
//! is std-only: no network, no clock, no hardware. Every input — watts, amps,
//! volts, elapsed seconds, hours remaining — is supplied by the caller.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`mode`] — the four charge modes ([`ChargeMode`]).
//! - [`balance`] — the power-balance model: spare-sunshine ([`surplus`])
//!   computation with a home-battery policy.
//! - [`current`] — turning spare watts into a clamped charge current, with the
//!   `PvOnly` pause and `MinPlusPv` grid-floor rules.
//! - [`phase`] — one-phase vs three-phase switching with a hysteresis margin.
//! - [`antiflap`] — the dwell timer that stops cloud-flapping.
//! - [`plan`] — the deadline planner: will the sun make it, or is a grid
//!   top-up needed?
//! - [`label`] — the EN / DE / TR grandma-friendly status surface (Charter
//!   §6.3, ADR-007).
//!
//! The **charger / wallbox adapters** (OCPP, go-eCharger, Wallbe, Easee, Tesla,
//! KEBA, Modbus EVSE), the **vehicle state-of-charge** and **meter** adapters,
//! the **cave-home-core / cave-home-history integration**, and **tariff /
//! price-based charging** are network / hardware / clock-bound and are deferred
//! to Phase 1b — every one is enumerated in `parity.manifest.toml`
//! `[[unmapped]]`. They feed their readings (as watts, amps, percent) into this
//! engine and reuse it unchanged.
//!
//! # Example
//!
//! ```
//! use cave_home_solar_evcc::{
//!     decide, BatteryPolicy, ChargeMode, CurrentLimits, PhaseCount, PowerSnapshot,
//!     ChargeStatus, Lang,
//! };
//!
//! // Sunny afternoon: 5 kW from the roof, the house using 1 kW, car not yet on.
//! let snapshot = PowerSnapshot::new(5000.0, 1000.0, 0.0, 0.0).unwrap();
//!
//! let outcome = decide(
//!     &snapshot,
//!     ChargeMode::PvOnly,
//!     CurrentLimits::typical_16a(),
//!     230.0,
//!     PhaseCount::Single,
//!     BatteryPolicy::HoldForHouse,
//! )
//! .unwrap();
//!
//! assert!(outcome.setpoint.is_charging());
//! assert_eq!(outcome.status, ChargeStatus::ChargingFromSun);
//! println!("{}", outcome.status.message(Lang::En)); // "Charging your car from the sun"
//! ```

#![allow(clippy::module_name_repetitions)]

pub mod antiflap;
pub mod balance;
pub mod current;
pub mod error;
pub mod label;
pub mod mode;
pub mod phase;
pub mod plan;

pub use antiflap::AntiFlapTimer;
pub use balance::{BatteryPolicy, PowerSnapshot};
pub use current::{
    current_for_surplus, decide_current, power_for_current, ChargeSetpoint, CurrentLimits,
};
pub use error::EvccError;
pub use label::{ChargeStatus, Lang};
pub use mode::ChargeMode;
pub use phase::{decide_phases, PhaseCount};
pub use plan::{plan, ChargePlan, PlanInputs, PlanOutcome};

/// The result of a single charge-control decision: the electrical setpoint plus
/// the plain-language status the household sees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChargeDecision {
    /// The electrical instruction for the charger.
    pub setpoint: ChargeSetpoint,
    /// The grandma-friendly status for the UI.
    pub status: ChargeStatus,
    /// The spare-sunshine surplus the decision was based on, in watts.
    pub surplus_watts: f64,
}

/// Decide what to do with the car this instant, end to end.
///
/// Computes the spare sunshine from the [`PowerSnapshot`], sizes the charge
/// current for the given [`ChargeMode`] / [`CurrentLimits`] / phase count, and
/// maps the result to a [`ChargeStatus`] in home language.
///
/// This is the convenience surface; callers that want phase switching,
/// anti-flap dwell or deadline planning compose [`decide_phases`],
/// [`AntiFlapTimer`] and [`plan`] around it.
///
/// # Errors
/// Propagates validation errors from [`decide_current`] (bad voltage / phase
/// count / non-finite input).
pub fn decide(
    snapshot: &PowerSnapshot,
    mode: ChargeMode,
    limits: CurrentLimits,
    voltage: f64,
    phases: PhaseCount,
    battery: BatteryPolicy,
) -> Result<ChargeDecision, EvccError> {
    let surplus_watts = snapshot.surplus_watts(battery);
    let setpoint = decide_current(surplus_watts, limits, voltage, phases.count(), mode)?;

    let status = match (mode, setpoint) {
        (ChargeMode::Off, _) => ChargeStatus::Off,
        (_, ChargeSetpoint::Paused) => ChargeStatus::PausedNotEnoughSun,
        (ChargeMode::Now, ChargeSetpoint::Charge { .. }) => ChargeStatus::ChargingFast,
        (ChargeMode::PvOnly, ChargeSetpoint::Charge { .. }) => ChargeStatus::ChargingFromSun,
        (ChargeMode::MinPlusPv, ChargeSetpoint::Charge { .. }) => {
            // If the surplus alone could not reach the charger minimum, the
            // grid is topping up the floor; otherwise it's riding the sun.
            if surplus_watts < limits.min_a() * voltage * f64::from(phases.count()) {
                ChargeStatus::ToppingUpForDeadline
            } else {
                ChargeStatus::ChargingFromSun
            }
        }
    };

    Ok(ChargeDecision { setpoint, status, surplus_watts })
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: f64 = 230.0;

    #[test]
    fn sunny_pv_only_charges_from_sun() {
        let snap = PowerSnapshot::new(5000.0, 1000.0, 0.0, 0.0).unwrap();
        let d = decide(
            &snap,
            ChargeMode::PvOnly,
            CurrentLimits::typical_16a(),
            V,
            PhaseCount::Single,
            BatteryPolicy::HoldForHouse,
        )
        .unwrap();
        assert!(d.setpoint.is_charging());
        assert_eq!(d.status, ChargeStatus::ChargingFromSun);
        assert!((d.surplus_watts - 4000.0).abs() < 1e-9);
    }

    #[test]
    fn cloudy_pv_only_pauses() {
        let snap = PowerSnapshot::new(900.0, 800.0, 0.0, 0.0).unwrap();
        let d = decide(
            &snap,
            ChargeMode::PvOnly,
            CurrentLimits::typical_16a(),
            V,
            PhaseCount::Single,
            BatteryPolicy::HoldForHouse,
        )
        .unwrap();
        assert_eq!(d.setpoint, ChargeSetpoint::Paused);
        assert_eq!(d.status, ChargeStatus::PausedNotEnoughSun);
    }

    #[test]
    fn min_plus_pv_short_sun_tops_up_from_grid() {
        // Deficit: house draws more than the roof makes; MinPlusPv still
        // charges at the minimum from the grid.
        let snap = PowerSnapshot::new(500.0, 1500.0, 0.0, 0.0).unwrap();
        let d = decide(
            &snap,
            ChargeMode::MinPlusPv,
            CurrentLimits::typical_16a(),
            V,
            PhaseCount::Single,
            BatteryPolicy::HoldForHouse,
        )
        .unwrap();
        assert!(d.setpoint.is_charging());
        assert!((d.setpoint.amps() - 6.0).abs() < 1e-9);
        assert_eq!(d.status, ChargeStatus::ToppingUpForDeadline);
    }

    #[test]
    fn now_mode_charges_fast_even_in_the_dark() {
        let snap = PowerSnapshot::new(0.0, 0.0, 0.0, 0.0).unwrap();
        let d = decide(
            &snap,
            ChargeMode::Now,
            CurrentLimits::typical_16a(),
            V,
            PhaseCount::Single,
            BatteryPolicy::HoldForHouse,
        )
        .unwrap();
        assert_eq!(d.status, ChargeStatus::ChargingFast);
        assert_eq!(d.setpoint.amps(), 16.0);
    }

    #[test]
    fn off_mode_reports_off() {
        let snap = PowerSnapshot::new(9000.0, 0.0, 0.0, 0.0).unwrap();
        let d = decide(
            &snap,
            ChargeMode::Off,
            CurrentLimits::typical_16a(),
            V,
            PhaseCount::Single,
            BatteryPolicy::HoldForHouse,
        )
        .unwrap();
        assert_eq!(d.status, ChargeStatus::Off);
        assert_eq!(d.setpoint, ChargeSetpoint::Paused);
    }

    #[test]
    fn decide_propagates_validation_errors() {
        let snap = PowerSnapshot::new(5000.0, 0.0, 0.0, 0.0).unwrap();
        let err = decide(
            &snap,
            ChargeMode::PvOnly,
            CurrentLimits::typical_16a(),
            0.0, // bad voltage
            PhaseCount::Single,
            BatteryPolicy::HoldForHouse,
        );
        assert_eq!(err, Err(EvccError::NonPositiveVoltage));
    }
}
