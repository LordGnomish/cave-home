// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0 core/site.go,
//         core/site_battery.go, core/site_circuits.go.
//
//! Site — top-level container that aggregates grid/PV/battery meters
//! and dispatches surplus to one or more `Loadpoint`s.
//!
//! The decision math in [`Site::compute_surplus`] is a faithful
//! port of upstream `SitePower`:
//!
//! ```text
//!     residual    = grid + battery + chargeCurrent - PV
//!     surplus     = -residual  (negative residual ⇒ export)
//!     hold_battery = SoC < min || forced
//! ```

use crate::error::{Error, Result};
use crate::loadpoint::Loadpoint;
use serde::{Deserialize, Serialize};

/// Battery operation mode. Direct port of upstream `api.BatteryMode`
/// (`unknown`, `normal`, `locked`, `hold`, `charge`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BatteryMode {
    /// `normal` — battery is free to charge from PV / discharge into the
    /// home as configured.
    Normal,
    /// `locked` — battery is forbidden from interacting with the home;
    /// upstream uses this during grid-supported planner sessions.
    Locked,
    /// `hold` — battery holds its present SoC (no charge, no discharge).
    /// Upstream picks this when a smart-cost charge session is active.
    Hold,
    /// `charge` — force-charge battery from grid, used by smart-cost
    /// planner during the cheapest tariff window.
    Charge,
}

/// Grid meter reading. Positive `power_w` ⇒ import; negative ⇒ export.
/// Matches the upstream sign convention exactly.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GridMeter {
    pub power_w: i32,
}

/// PV (solar) meter reading. Upstream uses unsigned `pvPower` (always ≥ 0
/// after `MeterPower` reads).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PvMeter {
    pub power_w: u32,
}

/// Home battery meter reading. Positive power ⇒ charging from PV /
/// grid; negative ⇒ discharging into the home.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BatteryMeter {
    pub power_w: i32,
    pub soc_percent: u8,
}

/// Residual + surplus pair as returned by [`Site::compute_surplus`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Surplus {
    /// Net power direction at the grid meter after the loadpoint draw.
    /// Positive ⇒ still importing (no surplus); negative ⇒ exporting.
    pub residual_w: i32,
    /// Available surplus power for additional loads in watts. Non-negative.
    pub available_w: u32,
}

/// Residual power explanation for the Portal/CLI Developer view —
/// breaks down the four components of `SitePower`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResidualPower {
    pub grid_w: i32,
    pub pv_w: u32,
    pub battery_w: i32,
    pub home_load_w: u32,
}

/// Site — aggregates grid + PV + battery + loadpoints.
#[derive(Debug, Clone)]
pub struct Site {
    pub name: String,
    pub grid: Option<GridMeter>,
    pub pv: Option<PvMeter>,
    pub battery: Option<BatteryMeter>,
    pub battery_mode: BatteryMode,
    /// Below this SoC the battery is held (no discharge into surplus).
    /// Upstream: `Site.BatteryMinSoc`.
    pub battery_min_soc_percent: u8,
    pub loadpoints: Vec<Loadpoint>,
    /// Maximum residual power, in watts, that can be drawn from the
    /// grid before the loadpoint must back off. Upstream:
    /// `Site.ResidualPower`.
    pub residual_power_buffer_w: i32,
}

impl Site {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            grid: None,
            pv: None,
            battery: None,
            battery_mode: BatteryMode::Normal,
            battery_min_soc_percent: 20,
            loadpoints: Vec::new(),
            residual_power_buffer_w: 100,
        }
    }

    pub fn set_grid(&mut self, m: GridMeter) {
        self.grid = Some(m);
    }
    pub fn set_pv(&mut self, m: PvMeter) {
        self.pv = Some(m);
    }
    pub fn set_battery(&mut self, m: BatteryMeter) -> Result<()> {
        if m.soc_percent > 100 {
            return Err(Error::InvalidSoc(i16::from(m.soc_percent)));
        }
        self.battery = Some(m);
        Ok(())
    }
    pub fn add_loadpoint(&mut self, lp: Loadpoint) {
        self.loadpoints.push(lp);
    }

    /// Compute the surplus power available to loadpoints, given the
    /// present meter readings.
    ///
    /// Source: upstream `core/site.go::SitePower`. The upstream signature
    /// returns `(sitePower, batteryBuffered, batteryStart)`; we collapse
    /// to the surplus-facing scalar that loadpoints actually consume.
    pub fn compute_surplus(&self) -> Result<Surplus> {
        let grid = self.grid.ok_or(Error::MeterUnavailable("grid"))?.power_w;
        let pv = self.pv.ok_or(Error::MeterUnavailable("pv"))?.power_w as i32;

        let (battery_w, hold_battery) = self.battery.map_or((0, false), |b| {
            let hold = matches!(self.battery_mode, BatteryMode::Hold | BatteryMode::Locked)
                || b.soc_percent < self.battery_min_soc_percent;
            (b.power_w, hold)
        });

        // Residual: how much we're still pulling from the grid AFTER the
        // PV / battery / home load have settled. Negative ⇒ we have
        // surplus to export. The upstream `SitePower` adds back the
        // (configurable) residual buffer so we never accidentally
        // export every last watt.
        //
        // If the battery is "held" the residual must subtract any
        // discharge contribution (so a discharging battery isn't read
        // as PV surplus). Upstream: `core/site_battery.go::batteryHold`.
        let effective_battery = if hold_battery { 0 } else { battery_w };
        let residual = grid + effective_battery + self.residual_power_buffer_w;

        let available = if residual < 0 {
            // Exporting: report magnitude as surplus.
            (-residual) as u32
        } else if pv > grid {
            // Net importer but PV producing more than grid imports — the
            // delta is locally consumed surplus.
            (pv - grid).max(0) as u32
        } else {
            0
        };

        Ok(Surplus {
            residual_w: residual,
            available_w: available,
        })
    }

    /// Breakdown of meter readings into Developer-view residual fields.
    /// Not used in the surplus loop, only for the Portal Developer view
    /// (Charter §6.3).
    #[must_use]
    pub fn residual_breakdown(&self) -> ResidualPower {
        let grid = self.grid.map(|g| g.power_w).unwrap_or(0);
        let pv = self.pv.map(|p| p.power_w).unwrap_or(0);
        let battery = self.battery.map(|b| b.power_w).unwrap_or(0);
        // House load reconstruction: house = grid + PV − battery_charge − loadpoint_draw.
        let lp_draw_w: u32 = self
            .loadpoints
            .iter()
            .map(|lp| {
                lp.charge_current_a.map_or(0, |a| {
                    crate::loadpoint::Loadpoint::current_to_watts(a, lp.phases)
                })
            })
            .sum();
        // Net "the rest of the house" load.
        let home_load_w = (grid.max(0) as u32 + pv).saturating_sub(lp_draw_w);
        ResidualPower {
            grid_w: grid,
            pv_w: pv,
            battery_w: battery,
            home_load_w,
        }
    }

    /// Convenience: total instantaneous loadpoint draw in watts.
    #[must_use]
    pub fn loadpoint_draw_w(&self) -> u32 {
        self.loadpoints
            .iter()
            .map(|lp| {
                lp.charge_current_a.map_or(0, |a| {
                    crate::loadpoint::Loadpoint::current_to_watts(a, lp.phases)
                })
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadpoint::{ChargeMode, Loadpoint, PhaseCount};

    fn site_with_meters(grid: i32, pv: u32, batt: i32, soc: u8) -> Site {
        let mut s = Site::new("home");
        s.set_grid(GridMeter { power_w: grid });
        s.set_pv(PvMeter { power_w: pv });
        s.set_battery(BatteryMeter {
            power_w: batt,
            soc_percent: soc,
        })
        .unwrap();
        s
    }

    #[test]
    fn no_grid_meter_yields_meter_unavailable() {
        let s = Site::new("home");
        assert!(matches!(s.compute_surplus(), Err(Error::MeterUnavailable("grid"))));
    }

    #[test]
    fn no_pv_meter_yields_meter_unavailable() {
        let mut s = Site::new("home");
        s.set_grid(GridMeter { power_w: 100 });
        assert!(matches!(s.compute_surplus(), Err(Error::MeterUnavailable("pv"))));
    }

    #[test]
    fn exporting_surplus_reports_magnitude() {
        // Grid -5 kW (exporting), PV 8 kW, no battery.
        let mut s = Site::new("home");
        s.set_grid(GridMeter { power_w: -5_000 });
        s.set_pv(PvMeter { power_w: 8_000 });
        let surplus = s.compute_surplus().unwrap();
        assert!(surplus.residual_w < 0);
        assert_eq!(surplus.available_w, (5_000 - 100) as u32); // minus buffer
    }

    #[test]
    fn importing_no_surplus() {
        let mut s = Site::new("home");
        s.set_grid(GridMeter { power_w: 2_000 });
        s.set_pv(PvMeter { power_w: 1_000 });
        let surplus = s.compute_surplus().unwrap();
        assert_eq!(surplus.available_w, 0);
    }

    #[test]
    fn battery_held_when_soc_below_min() {
        // SoC 15% < min 20% ⇒ battery discharge ignored even if helpful.
        let mut s = site_with_meters(-1_000, 5_000, -2_000, 15);
        s.battery_min_soc_percent = 20;
        let surplus = s.compute_surplus().unwrap();
        // residual = -1000 + 0 + 100 = -900 ⇒ available 900
        assert_eq!(surplus.residual_w, -900);
        assert_eq!(surplus.available_w, 900);
    }

    #[test]
    fn battery_discharge_contributes_to_residual_when_normal() {
        // SoC 80% above min, battery discharging at -2 kW.
        let mut s = site_with_meters(-1_000, 5_000, -2_000, 80);
        s.battery_min_soc_percent = 20;
        let surplus = s.compute_surplus().unwrap();
        // residual = -1000 + -2000 + 100 = -2900 ⇒ available 2900
        assert_eq!(surplus.residual_w, -2_900);
        assert_eq!(surplus.available_w, 2_900);
    }

    #[test]
    fn battery_locked_treated_as_held() {
        let mut s = site_with_meters(-1_000, 5_000, -2_000, 80);
        s.battery_mode = BatteryMode::Locked;
        let surplus = s.compute_surplus().unwrap();
        assert_eq!(surplus.residual_w, -900);
    }

    #[test]
    fn loadpoint_draw_aggregates_phase_current() {
        let mut s = Site::new("home");
        let mut lp = Loadpoint::new_ev("lp1");
        lp.set_charge_current(16).unwrap();
        s.add_loadpoint(lp);
        assert_eq!(s.loadpoint_draw_w(), 230 * 16 * 3);
    }

    #[test]
    fn residual_breakdown_exposes_all_components() {
        let mut s = site_with_meters(500, 4_000, 1_000, 60);
        let mut lp = Loadpoint::new_ev("lp1");
        lp.set_charge_current(10).unwrap();
        s.add_loadpoint(lp);
        let r = s.residual_breakdown();
        assert_eq!(r.grid_w, 500);
        assert_eq!(r.pv_w, 4_000);
        assert_eq!(r.battery_w, 1_000);
    }

    #[test]
    fn invalid_soc_rejected_on_set() {
        let mut s = Site::new("home");
        let r = s.set_battery(BatteryMeter {
            power_w: 0,
            soc_percent: 101,
        });
        assert!(r.is_err());
    }

    #[test]
    fn site_with_pv_mode_loadpoint_makes_decisions() {
        // 6 kW surplus on three phase ⇒ ~8 A ⇒ rounds down.
        let mut s = site_with_meters(-6_000, 8_000, 0, 80);
        let mut lp = Loadpoint::new_ev("lp1");
        lp.set_mode(ChargeMode::Pv);
        s.add_loadpoint(lp);
        let surplus = s.compute_surplus().unwrap();
        // residual = -6000 + 0 + 100 = -5900 ⇒ 5900 W
        // 5900 / (230*3) = 8 A
        let next = s.loadpoints[0].next_decision(surplus.available_w as i32);
        assert_eq!(next, 8);
    }

    #[test]
    fn three_phase_to_single_phase_helps_low_pv() {
        // Only 2.2 kW surplus — no three-phase floor possible, but single-phase 6 A works.
        let mut s = site_with_meters(-2_300, 3_000, 0, 80);
        let mut lp = Loadpoint::new_ev("lp1");
        lp.set_mode(ChargeMode::Pv);
        lp.set_phases(PhaseCount::Single).unwrap();
        s.add_loadpoint(lp);
        let surplus = s.compute_surplus().unwrap();
        let next = s.loadpoints[0].next_decision(surplus.available_w as i32);
        assert!(next >= 6);
    }
}
