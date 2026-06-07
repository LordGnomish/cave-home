// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The home battery state.

use crate::fleet_api::types::LiveStatus;

/// The home battery's energy and power state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatteryData {
    /// State of charge, percent.
    pub soc_percent: f64,
    /// Usable energy remaining, watt-hours.
    pub energy_remaining_wh: f64,
    /// Total usable capacity, watt-hours.
    pub total_capacity_wh: f64,
    /// Battery power, watts (negative = charging).
    pub power_watts: f64,
}

impl BatteryData {
    /// Whether the battery is charging.
    #[must_use]
    pub fn is_charging(&self) -> bool {
        self.power_watts < 0.0
    }

    /// Whether the battery is discharging.
    #[must_use]
    pub fn is_discharging(&self) -> bool {
        self.power_watts > 0.0
    }

    /// State of charge as a 0.0..=1.0 fraction (clamped).
    #[must_use]
    pub fn fraction(&self) -> f64 {
        (self.soc_percent / 100.0).clamp(0.0, 1.0)
    }
}

impl From<&LiveStatus> for BatteryData {
    fn from(s: &LiveStatus) -> Self {
        Self {
            soc_percent: s.percentage_charged,
            energy_remaining_wh: s.energy_left,
            total_capacity_wh: s.total_pack_energy,
            power_watts: s.battery_power,
        }
    }
}

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
