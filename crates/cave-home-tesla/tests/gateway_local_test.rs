// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! End-to-end Powerwall local-gateway flow through the public surface.

use cave_home_tesla::fleet_api::client::MockTransport;
use cave_home_tesla::gateway_local::{login_body, GatewayClient, LoginRequest};

#[test]
fn login_body_is_well_formed() {
    let body = login_body(&LoginRequest::installer_default("ABC12"));
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["username"], "customer");
    assert_eq!(v["password"], "ABC12");
    assert_eq!(v["force_sm_off"], false);
}

#[tokio::test]
async fn reads_power_flow_from_lan() {
    let t = MockTransport::new()
        .route(
            "/api/meters/aggregates",
            200,
            r#"{"site":{"instant_power":-500},"battery":{"instant_power":1500},
                "load":{"instant_power":1000},"solar":{"instant_power":0}}"#,
        )
        .route("/api/system_status/soe", 200, r#"{"percentage":42.0}"#);
    let gw = GatewayClient::new(t, "https://192.168.1.10").with_token("gw-token");

    let flow = gw.get_power_flow().await.unwrap();
    assert!((flow.soc_percent - 42.0).abs() < f64::EPSILON);
    assert!(flow.battery_discharging());
    assert!(flow.grid_exporting());
    assert!((flow.load_watts - 1000.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn surfaces_gateway_errors() {
    let t = MockTransport::new()
        .route("/api/meters/aggregates", 500, "gateway error")
        .route("/api/system_status/soe", 200, r#"{"percentage":0}"#);
    let gw = GatewayClient::new(t, "https://192.168.1.10").with_token("gw-token");
    assert!(gw.get_power_flow().await.is_err());
}
