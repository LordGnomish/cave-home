// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The instantaneous home power flow.

use crate::fleet_api::types::LiveStatus;

/// A snapshot of where power is flowing right now.
///
/// All fields are watts except `soc_percent`. Sign conventions follow Tesla:
/// `battery_watts` is negative while charging, `grid_watts` is negative while
/// exporting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerFlowData {
    /// Solar production.
    pub pv_watts: f64,
    /// Battery power (negative = charging).
    pub battery_watts: f64,
    /// House load.
    pub load_watts: f64,
    /// Grid power (negative = exporting).
    pub grid_watts: f64,
    /// State of charge, percent.
    pub soc_percent: f64,
}

impl PowerFlowData {
    /// Whether the home is drawing from the grid.
    #[must_use]
    pub fn grid_importing(&self) -> bool {
        self.grid_watts > 0.0
    }

    /// Whether the home is exporting to the grid.
    #[must_use]
    pub fn grid_exporting(&self) -> bool {
        self.grid_watts < 0.0
    }

    /// Grid import, watts (0 when exporting).
    #[must_use]
    pub const fn grid_import_watts(&self) -> f64 {
        self.grid_watts.max(0.0)
    }

    /// Grid export, watts (0 when importing).
    #[must_use]
    pub fn grid_export_watts(&self) -> f64 {
        (-self.grid_watts).max(0.0)
    }

    /// Whether the battery is charging.
    #[must_use]
    pub fn battery_charging(&self) -> bool {
        self.battery_watts < 0.0
    }

    /// Whether the battery is discharging.
    #[must_use]
    pub fn battery_discharging(&self) -> bool {
        self.battery_watts > 0.0
    }
}

impl From<&LiveStatus> for PowerFlowData {
    fn from(s: &LiveStatus) -> Self {
        Self {
            pv_watts: s.solar_power,
            battery_watts: s.battery_power,
            load_watts: s.load_power,
            grid_watts: s.grid_power,
            soc_percent: s.percentage_charged,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet_api::types::{Envelope, LiveStatus};

    fn live(solar: f64, battery: f64, load: f64, grid: f64, soc: f64) -> LiveStatus {
        let json = format!(
            r#"{{"response":{{"solar_power":{solar},"battery_power":{battery},
            "load_power":{load},"grid_power":{grid},"percentage_charged":{soc},
            "energy_left":0,"total_pack_energy":0,"grid_status":"Active"}}}}"#
        );
        serde_json::from_str::<Envelope<LiveStatus>>(&json).unwrap().response
    }

    #[test]
    fn maps_from_live_status() {
        let f = PowerFlowData::from(&live(3000.0, -1000.0, 2000.0, 0.0, 80.0));
        assert!((f.pv_watts - 3000.0).abs() < f64::EPSILON);
        assert!((f.battery_watts - -1000.0).abs() < f64::EPSILON);
        assert!((f.load_watts - 2000.0).abs() < f64::EPSILON);
        assert!((f.soc_percent - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn grid_direction_split() {
        let importing = PowerFlowData::from(&live(0.0, 0.0, 1500.0, 1500.0, 50.0));
        assert!(importing.grid_importing());
        assert!(!importing.grid_exporting());
        assert!((importing.grid_import_watts() - 1500.0).abs() < f64::EPSILON);
        assert!(importing.grid_export_watts().abs() < f64::EPSILON);

        let exporting = PowerFlowData::from(&live(5000.0, 0.0, 1000.0, -4000.0, 100.0));
        assert!(exporting.grid_exporting());
        assert!(!exporting.grid_importing());
        assert!((exporting.grid_export_watts() - 4000.0).abs() < f64::EPSILON);
        assert!(exporting.grid_import_watts().abs() < f64::EPSILON);
    }

    #[test]
    fn battery_direction_split() {
        let charging = PowerFlowData::from(&live(5000.0, -2000.0, 1000.0, 0.0, 60.0));
        assert!(charging.battery_charging());
        assert!(!charging.battery_discharging());

        let discharging = PowerFlowData::from(&live(0.0, 1500.0, 1500.0, 0.0, 40.0));
        assert!(discharging.battery_discharging());
        assert!(!discharging.battery_charging());
    }
}
