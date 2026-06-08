//! Power balance — how much spare sunshine is there to charge your car?
//!
//! Every input is supplied by the caller in **watts** (the meter / inverter
//! adapters that read real hardware are deferred to Phase 1b — see the parity
//! manifest). The engine itself does no I/O: it takes a snapshot of the house
//! and computes the surplus available to the car.
//!
//! Sign conventions (all watts, all caller-supplied):
//! - `pv_production` ≥ 0 — what the solar panels are making.
//! - `house_consumption` ≥ 0 — **total** house load *including* whatever the
//!   car is currently drawing (this is what a house meter reads).
//! - `battery_flow` — the home battery: **positive = charging** (taking power
//!   out of the house), **negative = discharging** (adding power back).
//! - `charge_power` ≥ 0 — what the car is drawing right now. It is *added back*
//!   to the surplus so the figure is "spare power as if the car were unplugged"
//!   — without this the setpoint would chase its own draw down to zero.

use crate::error::EvccError;

/// Whether the home battery may lend its discharge to car charging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryPolicy {
    /// Keep the home battery for the house; never count its discharge as
    /// surplus for the car. Battery charging still counts as house load.
    HoldForHouse,
    /// Let the home battery discharge into the car when it is already
    /// discharging (treat that discharge as available surplus).
    AssistCar,
}

/// A snapshot of the home's power flows, in watts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerSnapshot {
    pv_production: f64,
    house_consumption: f64,
    /// Positive = home battery charging; negative = discharging.
    battery_flow: f64,
    charge_power: f64,
}

impl PowerSnapshot {
    /// Build a validated snapshot.
    ///
    /// # Errors
    /// Returns [`EvccError::NotFinite`] for any non-finite watt value, and
    /// [`EvccError::NegativePower`] if production, consumption or charge power
    /// are below zero (battery flow may legitimately be negative).
    pub fn new(
        pv_production: f64,
        house_consumption: f64,
        battery_flow: f64,
        charge_power: f64,
    ) -> Result<Self, EvccError> {
        for v in [pv_production, house_consumption, battery_flow, charge_power] {
            if !v.is_finite() {
                return Err(EvccError::NotFinite);
            }
        }
        if pv_production < 0.0 || house_consumption < 0.0 || charge_power < 0.0 {
            return Err(EvccError::NegativePower);
        }
        Ok(Self { pv_production, house_consumption, battery_flow, charge_power })
    }

    #[must_use]
    pub const fn pv_production(&self) -> f64 {
        self.pv_production
    }

    #[must_use]
    pub const fn house_consumption(&self) -> f64 {
        self.house_consumption
    }

    #[must_use]
    pub const fn battery_flow(&self) -> f64 {
        self.battery_flow
    }

    #[must_use]
    pub const fn charge_power(&self) -> f64 {
        self.charge_power
    }

    /// The spare sunshine available to the car, in watts.
    ///
    /// The car's own current draw (`charge_power`) is **added back** so the
    /// figure is the surplus as if the car were not yet charging — this is
    /// what the current/phase logic needs to size the *next* setpoint without
    /// chasing its own tail.
    ///
    /// Under [`BatteryPolicy::HoldForHouse`] a *charging* home battery counts
    /// as house load (it is consuming surplus that could otherwise go to the
    /// car), but a *discharging* battery is ignored — we won't drain the house
    /// battery into the car. Under [`BatteryPolicy::AssistCar`] a discharging
    /// battery's output is added to the surplus.
    ///
    /// Never returns a negative surplus: a deficit is reported as `0.0`.
    #[must_use]
    pub fn surplus_watts(&self, policy: BatteryPolicy) -> f64 {
        // House load excluding the car; the car's draw is excluded by adding it
        // back below.
        let mut available = self.pv_production - self.house_consumption + self.charge_power;

        // battery_flow > 0 means the battery is charging => it is part of house
        // load and already subtracted via house_consumption *only if* the
        // caller folded it in. We model it explicitly here instead: a charging
        // battery removes that much from the car's surplus.
        if self.battery_flow > 0.0 {
            available -= self.battery_flow;
        } else if matches!(policy, BatteryPolicy::AssistCar) {
            // battery_flow <= 0 means discharging; magnitude assists the car.
            available += -self.battery_flow;
        }

        available.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(pv: f64, house: f64, batt: f64, charge: f64) -> PowerSnapshot {
        PowerSnapshot::new(pv, house, batt, charge).expect("valid snapshot")
    }

    #[test]
    fn surplus_is_pv_minus_house() {
        let s = snap(5000.0, 1200.0, 0.0, 0.0);
        assert_eq!(s.surplus_watts(BatteryPolicy::HoldForHouse), 3800.0);
    }

    #[test]
    fn car_draw_is_added_back_so_setpoint_does_not_chase_itself() {
        // PV 5kW, total house load 3.2kW (1.2kW house + 2kW the car is pulling),
        // car drawing 2kW -> spare as if unplugged is 5 - 3.2 + 2 = 3.8kW,
        // matching the 3.8kW the no-car case sees.
        let s = snap(5000.0, 3200.0, 0.0, 2000.0);
        assert_eq!(s.surplus_watts(BatteryPolicy::HoldForHouse), 3800.0);
    }

    #[test]
    fn deficit_clamps_to_zero() {
        let s = snap(800.0, 2000.0, 0.0, 0.0);
        assert_eq!(s.surplus_watts(BatteryPolicy::HoldForHouse), 0.0);
    }

    #[test]
    fn charging_battery_reduces_surplus() {
        // PV 5kW, house 1kW, battery soaking 2kW -> only 2kW left for the car.
        let s = snap(5000.0, 1000.0, 2000.0, 0.0);
        assert_eq!(s.surplus_watts(BatteryPolicy::HoldForHouse), 2000.0);
    }

    #[test]
    fn discharging_battery_held_for_house_does_not_help_car() {
        // Battery discharging 2kW; HoldForHouse ignores it.
        let s = snap(1000.0, 1500.0, -2000.0, 0.0);
        assert_eq!(s.surplus_watts(BatteryPolicy::HoldForHouse), 0.0);
    }

    #[test]
    fn discharging_battery_assist_car_adds_to_surplus() {
        // PV 1kW, house 1.5kW (deficit 0.5kW), battery lending 2kW.
        let s = snap(1000.0, 1500.0, -2000.0, 0.0);
        assert_eq!(s.surplus_watts(BatteryPolicy::AssistCar), 1500.0);
    }

    #[test]
    fn rejects_non_finite() {
        assert_eq!(
            PowerSnapshot::new(f64::NAN, 0.0, 0.0, 0.0),
            Err(EvccError::NotFinite)
        );
        assert_eq!(
            PowerSnapshot::new(0.0, 0.0, f64::INFINITY, 0.0),
            Err(EvccError::NotFinite)
        );
    }

    #[test]
    fn rejects_negative_production() {
        assert_eq!(
            PowerSnapshot::new(-1.0, 0.0, 0.0, 0.0),
            Err(EvccError::NegativePower)
        );
        assert_eq!(
            PowerSnapshot::new(0.0, 0.0, 0.0, -1.0),
            Err(EvccError::NegativePower)
        );
    }

    #[test]
    fn battery_may_be_negative() {
        assert!(PowerSnapshot::new(0.0, 0.0, -3000.0, 0.0).is_ok());
    }
}
