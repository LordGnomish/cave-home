// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! End-to-end Fleet API flow through the public `EnergyProvider` surface,
//! driven by the in-crate `MockTransport` (the stand-in for the deferred real
//! HTTP transport).

use std::sync::Arc;

use cave_home_tesla::fleet_api::client::{FleetClient, ManualClock, MockTransport};
use cave_home_tesla::fleet_api::Region;
use cave_home_tesla::metrics::Metrics;
use cave_home_tesla::{DateRange, EnergyProvider, OpMode, TeslaProvider};

const LIVE: &str = r#"{"response":{"solar_power":4200,"battery_power":-1500,"load_power":1800,
    "grid_power":-900,"percentage_charged":88,"energy_left":11880,"total_pack_energy":13500,
    "grid_status":"Active","island_status":"on_grid","storm_mode_active":false,
    "backup_capable":true}}"#;
const INFO: &str = r#"{"response":{"id":"STE42","site_name":"Cave","backup_reserve_percent":20,
    "default_real_mode":"autonomous","battery_count":2,"nameplate_power":10000,
    "nameplate_energy":27000}}"#;
const HIST: &str = r#"{"response":{"period":"day","time_series":[
    {"timestamp":"2026-06-07T08:00:00-07:00","solar_power":1200,"battery_power":-1200,"grid_power":0},
    {"timestamp":"2026-06-07T12:00:00-07:00","solar_power":6800,"battery_power":-2000,"grid_power":-3000}]}}"#;

fn provider() -> (TeslaProvider<MockTransport>, Arc<ManualClock>) {
    let t = MockTransport::new()
        .route("/live_status", 200, LIVE)
        .route("/site_info", 200, INFO)
        .route("/calendar_history", 200, HIST)
        .route("/backup", 200, r#"{"response":{"result":true}}"#)
        .route("/operation", 200, r#"{"response":{"result":true}}"#);
    let clock = Arc::new(ManualClock::new(0));
    let client = FleetClient::new(t, Region::Europe).with_token("AT-test");
    (TeslaProvider::new(client, 42, clock.clone()).with_retries(0), clock)
}

#[tokio::test]
async fn reads_power_flow_and_feeds_metrics() {
    let (p, _clock) = provider();
    let flow = p.get_power_flow().await.unwrap();
    assert!((flow.pv_watts - 4200.0).abs() < f64::EPSILON);
    assert!(flow.grid_exporting());
    assert!((flow.grid_export_watts() - 900.0).abs() < f64::EPSILON);

    let metrics = Metrics::new();
    metrics.record_power_flow(&flow);
    let exposition = metrics.render();
    assert!(exposition.contains("tesla_pv_power_watts 4200"));
    assert!(exposition.contains("tesla_grid_export_watts 900"));
    assert!(exposition.contains("tesla_battery_soc_percent 88"));
}

#[tokio::test]
async fn reads_full_status() {
    let (p, _clock) = provider();
    let s = p.get_status().await.unwrap();
    assert_eq!(s.name.as_deref(), Some("Cave"));
    assert!(s.grid_connected);
    assert_eq!(s.op_mode, Some(OpMode::Autonomous));
    assert_eq!(s.backup_reserve_percent, Some(20));
}

#[tokio::test]
async fn commands_backup_reserve_and_mode() {
    let (p, _clock) = provider();
    p.set_backup_reserve(30).await.unwrap();
    p.set_operation_mode(OpMode::SelfConsumption).await.unwrap();
    let reqs = p.client().transport().requests.lock();
    assert!(reqs.iter().any(|r| r.url.ends_with("/backup")));
    assert!(reqs
        .iter()
        .any(|r| r.body.as_deref().unwrap_or("").contains("self_consumption")));
}

#[tokio::test]
async fn reads_history_over_24h() {
    let (p, _clock) = provider();
    let range = DateRange::last_hours(1_000_000, 24);
    let h = p.get_history(range).await.unwrap();
    assert_eq!(h.samples.len(), 2);
    assert!((h.peak_pv_watts() - 6800.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn serves_cache_through_a_cloud_blip() {
    let (p, clock) = provider();
    // Prime, then knock the cloud over within the cache TTL.
    let _ = p.get_power_flow().await.unwrap();
    p.client().transport().set_failure(Some("cloud down".into()));
    clock.advance(60_000);
    let flow = p.get_power_flow().await.unwrap();
    assert!((flow.pv_watts - 4200.0).abs() < f64::EPSILON);
}
