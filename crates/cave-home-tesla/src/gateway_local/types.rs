// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Powerwall local-gateway wire DTOs.

use serde::Deserialize;

use crate::models::PowerFlowData;

/// One meter's reading. The gateway reports more fields; cave-home needs the
/// instantaneous power.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct MeterReading {
    /// Instantaneous power, watts.
    #[serde(default)]
    pub instant_power: f64,
}

/// `GET /api/meters/aggregates` — per-domain instantaneous power. Sign
/// conventions match the cloud: `battery` and `site` are negative while
/// charging / exporting respectively.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct MetersAggregates {
    /// The grid meter (negative = exporting).
    pub site: MeterReading,
    /// The battery meter (negative = charging).
    pub battery: MeterReading,
    /// The house load meter.
    pub load: MeterReading,
    /// The solar meter.
    pub solar: MeterReading,
}

impl MetersAggregates {
    /// Combine these meter readings with a state of charge into a
    /// [`PowerFlowData`].
    #[must_use]
    pub const fn power_flow(&self, soc_percent: f64) -> PowerFlowData {
        PowerFlowData {
            pv_watts: self.solar.instant_power,
            battery_watts: self.battery.instant_power,
            load_watts: self.load.instant_power,
            grid_watts: self.site.instant_power,
            soc_percent,
        }
    }
}

/// `GET /api/system_status/soe` — the aggregate state of energy.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct SystemSoe {
    /// State of charge, percent.
    #[serde(default)]
    pub percentage: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGGREGATES: &str = r#"{
      "site":    {"instant_power": 1200.0, "frequency": 50.0},
      "battery": {"instant_power": -1000.0, "energy_charged": 5000.0},
      "load":    {"instant_power": 2200.0},
      "solar":   {"instant_power": 2000.0}
    }"#;

    #[test]
    fn parse_aggregates() {
        let a: MetersAggregates = serde_json::from_str(AGGREGATES).unwrap();
        assert!((a.solar.instant_power - 2000.0).abs() < f64::EPSILON);
        assert!((a.battery.instant_power - -1000.0).abs() < f64::EPSILON);
        assert!((a.load.instant_power - 2200.0).abs() < f64::EPSILON);
        assert!((a.site.instant_power - 1200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_soe() {
        let s: SystemSoe = serde_json::from_str(r#"{"percentage": 56.5}"#).unwrap();
        assert!((s.percentage - 56.5).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregates_map_to_power_flow_with_soc() {
        let a: MetersAggregates = serde_json::from_str(AGGREGATES).unwrap();
        let f = a.power_flow(56.5);
        assert!((f.pv_watts - 2000.0).abs() < f64::EPSILON);
        assert!((f.grid_watts - 1200.0).abs() < f64::EPSILON);
        assert!((f.battery_watts - -1000.0).abs() < f64::EPSILON);
        assert!((f.load_watts - 2200.0).abs() < f64::EPSILON);
        assert!((f.soc_percent - 56.5).abs() < f64::EPSILON);
        assert!(f.battery_charging());
        assert!(f.grid_importing());
    }
}
