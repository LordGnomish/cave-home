// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Powerwall local-gateway wire DTOs.

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
