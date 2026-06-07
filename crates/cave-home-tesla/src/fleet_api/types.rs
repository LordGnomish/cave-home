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

use serde::Deserialize;

/// The `{ "response": ... }` envelope every Fleet API endpoint wraps its
/// payload in. `count` is present on list endpoints; cave-home ignores it.
#[derive(Debug, Clone, Deserialize)]
pub struct Envelope<T> {
    /// The endpoint payload.
    pub response: T,
}

/// `GET /api/1/energy_sites/{id}/live_status` — the instantaneous power flows
/// and state of charge.
///
/// All powers are watts; `battery_power` is negative while charging,
/// `grid_power` is negative while exporting.
#[derive(Debug, Clone, Deserialize)]
pub struct LiveStatus {
    /// Solar production, watts.
    #[serde(default)]
    pub solar_power: f64,
    /// Battery power, watts (negative = charging).
    #[serde(default)]
    pub battery_power: f64,
    /// House load, watts.
    #[serde(default)]
    pub load_power: f64,
    /// Grid power, watts (negative = exporting).
    #[serde(default)]
    pub grid_power: f64,
    /// State of charge, percent.
    #[serde(default)]
    pub percentage_charged: f64,
    /// Usable energy remaining, watt-hours.
    #[serde(default)]
    pub energy_left: f64,
    /// Total usable pack energy, watt-hours.
    #[serde(default)]
    pub total_pack_energy: f64,
    /// Grid connection status (`Active`, `Inactive`, …).
    #[serde(default)]
    pub grid_status: String,
    /// Microgrid island status (`on_grid`, `off_grid`, …), if reported.
    #[serde(default)]
    pub island_status: Option<String>,
    /// Whether storm watch is currently overriding the reserve.
    #[serde(default)]
    pub storm_mode_active: Option<bool>,
    /// Whether the site can supply backup power.
    #[serde(default)]
    pub backup_capable: Option<bool>,
    /// Server timestamp of the sample, if present.
    #[serde(default)]
    pub timestamp: Option<String>,
}

/// `GET /api/1/energy_sites/{id}/site_info` — the site's configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct SiteInfo {
    /// Opaque site id.
    #[serde(default)]
    pub id: Option<String>,
    /// Household-chosen site name.
    #[serde(default)]
    pub site_name: Option<String>,
    /// The configured backup reserve, percent.
    #[serde(default)]
    pub backup_reserve_percent: u8,
    /// The configured operation mode (`self_consumption`, `backup`,
    /// `autonomous`).
    #[serde(default)]
    pub default_real_mode: String,
    /// Number of battery units.
    #[serde(default)]
    pub battery_count: u32,
    /// Nameplate inverter power, watts.
    #[serde(default)]
    pub nameplate_power: f64,
    /// Nameplate usable energy, watt-hours.
    #[serde(default)]
    pub nameplate_energy: f64,
}

/// `GET /api/1/energy_sites/{id}/calendar_history?kind=power` — a time series.
#[derive(Debug, Clone, Deserialize)]
pub struct HistorySeries {
    /// The aggregation period (`day`, `week`, `month`, `year`).
    #[serde(default)]
    pub period: String,
    /// The samples, oldest first.
    #[serde(default)]
    pub time_series: Vec<HistoryPoint>,
}

/// One sample in a `kind=power` calendar history.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryPoint {
    /// ISO-8601 timestamp of the sample.
    #[serde(default)]
    pub timestamp: String,
    /// Solar production at the sample, watts.
    #[serde(default)]
    pub solar_power: f64,
    /// Battery power at the sample, watts (negative = charging).
    #[serde(default)]
    pub battery_power: f64,
    /// Grid power at the sample, watts (negative = exporting).
    #[serde(default)]
    pub grid_power: f64,
}

/// One entry in `GET /api/1/products`. The list mixes vehicles and energy
/// sites; only the latter carry an `energy_site_id`.
#[derive(Debug, Clone, Deserialize)]
pub struct Product {
    /// The energy site id, present only for energy products.
    #[serde(default)]
    pub energy_site_id: Option<u64>,
    /// The site name, for energy products.
    #[serde(default)]
    pub site_name: Option<String>,
    /// The resource type (`battery`, `solar`, …) for energy products.
    #[serde(default)]
    pub resource_type: Option<String>,
}

impl Product {
    /// The energy site id if this product is an energy site.
    #[must_use]
    pub const fn energy_site_id(&self) -> Option<u64> {
        self.energy_site_id
    }
}

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
