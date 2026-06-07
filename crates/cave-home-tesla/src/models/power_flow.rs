// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The instantaneous home power flow.

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
