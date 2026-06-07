// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Powerwall local-gateway client.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet_api::client::MockTransport;

    #[test]
    fn login_body_carries_credentials() {
        let body = login_body(&LoginRequest::installer_default("secret"));
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["username"], "customer");
        assert_eq!(v["password"], "secret");
    }

    #[tokio::test]
    async fn get_power_flow_combines_aggregates_and_soe() {
        let t = MockTransport::new()
            .route(
                "/api/meters/aggregates",
                200,
                r#"{"site":{"instant_power":1200},"battery":{"instant_power":-1000},
                    "load":{"instant_power":2200},"solar":{"instant_power":2000}}"#,
            )
            .route("/api/system_status/soe", 200, r#"{"percentage":56.5}"#);
        let gw = GatewayClient::new(t, "https://192.168.1.10").with_token("tok");
        let f = gw.get_power_flow().await.unwrap();
        assert!((f.pv_watts - 2000.0).abs() < f64::EPSILON);
        assert!((f.soc_percent - 56.5).abs() < f64::EPSILON);
        assert!(f.battery_charging());
    }

    #[tokio::test]
    async fn get_power_flow_maps_error_status() {
        let t = MockTransport::new()
            .route("/api/meters/aggregates", 502, "bad gateway")
            .route("/api/system_status/soe", 200, r#"{"percentage":50}"#);
        let gw = GatewayClient::new(t, "https://192.168.1.10").with_token("tok");
        assert!(gw.get_power_flow().await.is_err());
    }

    #[tokio::test]
    async fn requests_target_the_gateway_base() {
        let t = MockTransport::new()
            .route("/api/meters/aggregates", 200, r#"{"site":{"instant_power":0},
                "battery":{"instant_power":0},"load":{"instant_power":0},"solar":{"instant_power":0}}"#)
            .route("/api/system_status/soe", 200, r#"{"percentage":0}"#);
        let gw = GatewayClient::new(t, "https://powerwall.local").with_token("tok");
        gw.get_power_flow().await.unwrap();
        let reqs = gw.transport().requests.lock();
        assert!(reqs.iter().all(|r| r.url.starts_with("https://powerwall.local")));
    }
}
