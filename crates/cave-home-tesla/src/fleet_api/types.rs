// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Fleet API wire DTOs — the raw JSON shapes the energy endpoints return.
//!
//! These mirror Tesla's documented response bodies as closely as cave-home
//! needs. They are deliberately lenient (`#[serde(default)]` everywhere a field
//! is optional or version-dependent) so a firmware/API revision that adds or
//! drops a field still parses. The mapping into the clean domain model lives in
//! [`crate::models`].

#[cfg(test)]
mod tests {
    use super::*;

    const LIVE_STATUS: &str = r#"{
      "response": {
        "solar_power": 3450,
        "energy_left": 12300.5,
        "total_pack_energy": 13500,
        "percentage_charged": 91.11,
        "backup_capable": true,
        "battery_power": -1000,
        "load_power": 2450,
        "grid_power": 0,
        "grid_status": "Active",
        "grid_services_active": false,
        "island_status": "on_grid",
        "storm_mode_active": false,
        "timestamp": "2026-06-07T12:00:00-07:00"
      }
    }"#;

    #[test]
    fn parse_live_status_envelope() {
        let env: Envelope<LiveStatus> = serde_json::from_str(LIVE_STATUS).unwrap();
        let s = env.response;
        assert!((s.solar_power - 3450.0).abs() < f64::EPSILON);
        assert!((s.battery_power - -1000.0).abs() < f64::EPSILON);
        assert!((s.load_power - 2450.0).abs() < f64::EPSILON);
        assert!((s.percentage_charged - 91.11).abs() < 1e-6);
        assert_eq!(s.grid_status, "Active");
        assert_eq!(s.island_status.as_deref(), Some("on_grid"));
        assert_eq!(s.storm_mode_active, Some(false));
        assert_eq!(s.backup_capable, Some(true));
    }

    #[test]
    fn live_status_tolerates_missing_optionals() {
        let minimal = r#"{"response":{"solar_power":100,"battery_power":0,"load_power":100,
            "grid_power":0,"percentage_charged":50,"energy_left":0,"total_pack_energy":0,
            "grid_status":"Active"}}"#;
        let env: Envelope<LiveStatus> = serde_json::from_str(minimal).unwrap();
        assert!(env.response.island_status.is_none());
        assert!(env.response.storm_mode_active.is_none());
    }

    #[test]
    fn parse_site_info() {
        let json = r#"{"response":{
            "id":"STE123","site_name":"Cave","backup_reserve_percent":20,
            "default_real_mode":"self_consumption","battery_count":2,
            "nameplate_power":10000,"nameplate_energy":27000}}"#;
        let env: Envelope<SiteInfo> = serde_json::from_str(json).unwrap();
        let info = env.response;
        assert_eq!(info.site_name.as_deref(), Some("Cave"));
        assert_eq!(info.backup_reserve_percent, 20);
        assert_eq!(info.default_real_mode, "self_consumption");
        assert_eq!(info.battery_count, 2);
    }

    #[test]
    fn parse_calendar_history_power_series() {
        let json = r#"{"response":{"period":"day","time_series":[
            {"timestamp":"2026-06-07T00:00:00-07:00","solar_power":0,"battery_power":500,"grid_power":-500},
            {"timestamp":"2026-06-07T12:00:00-07:00","solar_power":4000,"battery_power":-1500,"grid_power":0}
        ]}}"#;
        let env: Envelope<HistorySeries> = serde_json::from_str(json).unwrap();
        assert_eq!(env.response.period, "day");
        assert_eq!(env.response.time_series.len(), 2);
        assert!((env.response.time_series[1].solar_power - 4000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_products_picks_energy_sites() {
        let json = r#"{"response":[
            {"vin":"5YJ...","display_name":"Car"},
            {"energy_site_id":1234567890,"site_name":"Home","resource_type":"battery"}
        ],"count":2}"#;
        let env: Envelope<Vec<Product>> = serde_json::from_str(json).unwrap();
        let sites: Vec<_> = env.response.iter().filter_map(Product::energy_site_id).collect();
        assert_eq!(sites, vec![1_234_567_890]);
    }
}
