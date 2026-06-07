// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The [`EnergyProvider`] trait and its Tesla implementation.
//!
//! [`TeslaProvider`] ties the [`FleetClient`](crate::fleet_api::client::FleetClient)
//! together with a [`StateCache`]: every read is parsed into the
//! [`crate::models`] domain model, cached, and — when the API is briefly
//! unreachable — served from that cache for up to a TTL (5 minutes by default)
//! so the household-facing surfaces keep working through a cloud blip.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::fleet_api::client::{FleetClient, ManualClock, MockTransport};
    use crate::fleet_api::Region;
    use crate::models::OpMode;

    const LIVE: &str = r#"{"response":{"solar_power":3000,"battery_power":-1000,"load_power":2000,
        "grid_power":0,"percentage_charged":80,"energy_left":10800,"total_pack_energy":13500,
        "grid_status":"Active","island_status":"on_grid","storm_mode_active":false}}"#;
    const INFO: &str = r#"{"response":{"id":"STE1","site_name":"Cave","backup_reserve_percent":20,
        "default_real_mode":"self_consumption","battery_count":1,"nameplate_power":5000,
        "nameplate_energy":13500}}"#;

    fn provider(t: MockTransport, clock: Arc<ManualClock>) -> TeslaProvider<MockTransport> {
        let client = FleetClient::new(t, Region::Europe).with_token("AT");
        TeslaProvider::new(client, 1, clock).with_retries(0)
    }

    #[tokio::test]
    async fn get_power_flow_parses_and_caches() {
        let clock = Arc::new(ManualClock::new(0));
        let p = provider(MockTransport::new().route("/live_status", 200, LIVE), clock);
        let f = p.get_power_flow().await.unwrap();
        assert!((f.pv_watts - 3000.0).abs() < f64::EPSILON);
        assert!(f.battery_charging());
        assert!((f.soc_percent - 80.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn get_power_flow_served_from_cache_on_failure() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new().route("/live_status", 200, LIVE);
        let p = provider(t, clock.clone());
        // Prime the cache.
        let _ = p.get_power_flow().await.unwrap();
        // Now the API goes dark; advance past the rate-limit but within TTL.
        p.client().transport().set_failure(Some("network down".into()));
        clock.advance(60_000);
        let f = p.get_power_flow().await.unwrap();
        assert!((f.pv_watts - 3000.0).abs() < f64::EPSILON, "served stale snapshot");
    }

    #[tokio::test]
    async fn get_power_flow_errors_when_failing_with_no_cache() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new();
        t.set_failure(Some("network down".into()));
        let p = provider(t, clock);
        assert!(p.get_power_flow().await.is_err());
    }

    #[tokio::test]
    async fn get_power_flow_errors_when_cache_is_stale() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new().route("/live_status", 200, LIVE);
        let p = provider(t, clock.clone()).with_cache_ttl_secs(300);
        let _ = p.get_power_flow().await.unwrap();
        p.client().transport().set_failure(Some("down".into()));
        // Past both the rate limit and the 5-minute TTL.
        clock.advance(301_000);
        assert!(p.get_power_flow().await.is_err());
    }

    #[tokio::test]
    async fn get_status_merges_live_and_info() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new()
            .route("/live_status", 200, LIVE)
            .route("/site_info", 200, INFO);
        let p = provider(t, clock);
        let s = p.get_status().await.unwrap();
        assert_eq!(s.name.as_deref(), Some("Cave"));
        assert!(s.grid_connected);
        assert_eq!(s.op_mode, Some(OpMode::SelfConsumption));
        assert_eq!(s.backup_reserve_percent, Some(20));
    }

    #[tokio::test]
    async fn set_backup_reserve_rejects_over_100() {
        let clock = Arc::new(ManualClock::new(0));
        let p = provider(MockTransport::new(), clock);
        let err = p.set_backup_reserve(150).await.unwrap_err();
        assert!(matches!(err, TeslaError::Validation(_)));
        // No request should have been attempted.
        assert_eq!(p.client().transport().request_count(), 0);
    }

    #[tokio::test]
    async fn set_backup_reserve_posts_command() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new().route("/backup", 200, r#"{"response":{"result":true}}"#);
        let p = provider(t, clock);
        p.set_backup_reserve(25).await.unwrap();
        let reqs = p.client().transport().requests.lock();
        assert!(reqs[0].body.as_deref().unwrap().contains("\"backup_reserve_percent\":25"));
    }

    #[tokio::test]
    async fn set_operation_mode_posts_wire_value() {
        let clock = Arc::new(ManualClock::new(0));
        let t = MockTransport::new().route("/operation", 200, r#"{"response":{"result":true}}"#);
        let p = provider(t, clock);
        p.set_operation_mode(OpMode::Backup).await.unwrap();
        let reqs = p.client().transport().requests.lock();
        assert!(reqs[0].body.as_deref().unwrap().contains("\"default_real_mode\":\"backup\""));
    }

    #[tokio::test]
    async fn get_history_maps_series() {
        let clock = Arc::new(ManualClock::new(0));
        let hist = r#"{"response":{"period":"day","time_series":[
            {"timestamp":"t0","solar_power":0,"battery_power":0,"grid_power":0},
            {"timestamp":"t1","solar_power":4200,"battery_power":0,"grid_power":0}]}}"#;
        let t = MockTransport::new().route("/calendar_history", 200, hist);
        let p = provider(t, clock);
        let range = DateRange::last_hours(1_000_000, 24);
        let h = p.get_history(range).await.unwrap();
        assert_eq!(h.samples.len(), 2);
        assert!((h.peak_pv_watts() - 4200.0).abs() < f64::EPSILON);
    }
}
