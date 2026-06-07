// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The home battery state.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet_api::types::{Envelope, LiveStatus};

    fn live() -> LiveStatus {
        let json = r#"{"response":{"solar_power":0,"battery_power":-1000,"load_power":500,
            "grid_power":0,"percentage_charged":75,"energy_left":10125,
            "total_pack_energy":13500,"grid_status":"Active"}}"#;
        serde_json::from_str::<Envelope<LiveStatus>>(json).unwrap().response
    }

    #[test]
    fn maps_from_live_status() {
        let b = BatteryData::from(&live());
        assert!((b.soc_percent - 75.0).abs() < f64::EPSILON);
        assert!((b.energy_remaining_wh - 10125.0).abs() < f64::EPSILON);
        assert!((b.total_capacity_wh - 13500.0).abs() < f64::EPSILON);
        assert!((b.power_watts - -1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn charging_flag_follows_sign() {
        let b = BatteryData::from(&live());
        assert!(b.is_charging());
        assert!(!b.is_discharging());
    }

    #[test]
    fn fraction_is_soc_over_100() {
        let b = BatteryData::from(&live());
        assert!((b.fraction() - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn fraction_is_clamped() {
        let mut b = BatteryData::from(&live());
        b.soc_percent = 130.0;
        assert!((b.fraction() - 1.0).abs() < f64::EPSILON);
        b.soc_percent = -5.0;
        assert!(b.fraction().abs() < f64::EPSILON);
    }
}
