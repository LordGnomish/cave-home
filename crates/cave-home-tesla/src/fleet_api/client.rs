// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The rate-limited, transport-injected Fleet API request model.
//!
//! The client owns the *decisions* — per-endpoint rate limiting (1 req / 30 s),
//! exponential 429 back-off, auth-header injection and status→error mapping —
//! over an injected [`HttpTransport`] and an injected [`Clock`]. The real
//! `reqwest`/TLS transport is the only deferred piece (Phase 1b); the tests
//! drive everything through [`MockTransport`].

#[cfg(test)]
mod tests {
    use super::super::endpoints::{EnergyEndpoint, HttpMethod};
    use super::super::Region;
    use super::*;

    #[test]
    fn rate_limiter_first_request_is_ready() {
        let rl = RateLimiter::with_interval_secs(30);
        assert_eq!(rl.check("live", 0), RateDecision::Ready);
    }

    #[test]
    fn rate_limiter_throttles_a_burst() {
        let rl = RateLimiter::with_interval_secs(30);
        assert_eq!(rl.check("live", 0), RateDecision::Ready);
        // Same endpoint, 1s later: must wait ~29s.
        assert_eq!(rl.check("live", 1_000), RateDecision::Wait(29_000));
        assert_eq!(rl.check("live", 29_999), RateDecision::Wait(1));
    }

    #[test]
    fn rate_limiter_recovers_after_interval() {
        let rl = RateLimiter::with_interval_secs(30);
        assert_eq!(rl.check("live", 0), RateDecision::Ready);
        assert_eq!(rl.check("live", 30_000), RateDecision::Ready);
    }

    #[test]
    fn rate_limiter_keys_are_independent() {
        let rl = RateLimiter::with_interval_secs(30);
        assert_eq!(rl.check("live", 0), RateDecision::Ready);
        assert_eq!(rl.check("info", 0), RateDecision::Ready);
    }

    #[test]
    fn backoff_doubles_up_to_a_cap() {
        let b = Backoff::new(1_000, 60_000);
        assert_eq!(b.delay_for_attempt(0), 1_000);
        assert_eq!(b.delay_for_attempt(1), 2_000);
        assert_eq!(b.delay_for_attempt(2), 4_000);
        assert_eq!(b.delay_for_attempt(6), 60_000); // 64s clamped to 60s cap
        assert_eq!(b.delay_for_attempt(100), 60_000);
    }

    #[test]
    fn manual_clock_advances() {
        let c = ManualClock::new(5);
        assert_eq!(c.now_millis(), 5);
        c.advance(10);
        assert_eq!(c.now_millis(), 15);
    }

    #[tokio::test]
    async fn mock_transport_routes_and_logs_requests() {
        let t = MockTransport::new().route("/live_status", 200, r#"{"ok":true}"#);
        let resp = t
            .send(HttpRequest::get("https://x/api/1/energy_sites/1/live_status"))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(t.request_count(), 1);
    }

    #[tokio::test]
    async fn client_call_returns_ok_body() {
        let t = MockTransport::new().route("/live_status", 200, r#"{"response":{}}"#);
        let client = FleetClient::new(t, Region::Europe).with_token("AT-123");
        let resp = client.call(&EnergyEndpoint::LiveStatus(1), 0).await.unwrap();
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn client_call_injects_bearer_header() {
        let t = MockTransport::new().route("/live_status", 200, "{}");
        let client = FleetClient::new(t, Region::Europe).with_token("AT-123");
        client.call(&EnergyEndpoint::LiveStatus(1), 0).await.unwrap();
        let reqs = client.transport().requests.lock();
        let auth = reqs[0]
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"));
        assert_eq!(auth.map(|(_, v)| v.as_str()), Some("Bearer AT-123"));
    }

    #[tokio::test]
    async fn client_call_maps_non_2xx_to_error() {
        let t = MockTransport::new().route("/live_status", 500, "boom");
        let client = FleetClient::new(t, Region::Europe).with_token("AT");
        let err = client.call(&EnergyEndpoint::LiveStatus(1), 0).await.unwrap_err();
        assert!(matches!(err, TeslaError::Http { status: 500, .. }));
    }

    #[tokio::test]
    async fn client_call_enforces_rate_limit() {
        let t = MockTransport::new().route("/live_status", 200, "{}");
        let client = FleetClient::new(t, Region::Europe).with_token("AT");
        client.call(&EnergyEndpoint::LiveStatus(1), 0).await.unwrap();
        // Second call to the same endpoint 1s later is throttled.
        let err = client
            .call(&EnergyEndpoint::LiveStatus(1), 1_000)
            .await
            .unwrap_err();
        assert!(matches!(err, TeslaError::RateLimited { .. }));
    }

    #[tokio::test]
    async fn client_post_sets_method_and_body() {
        let t = MockTransport::new().route("/backup", 200, "{}");
        let client = FleetClient::new(t, Region::Europe).with_token("AT");
        client
            .call(&EnergyEndpoint::SetBackupReserve { site_id: 1, percent: 30 }, 0)
            .await
            .unwrap();
        let reqs = client.transport().requests.lock();
        assert_eq!(reqs[0].method, HttpMethod::Post);
        assert!(reqs[0].body.as_deref().unwrap().contains("backup_reserve_percent"));
    }

    #[tokio::test(start_paused = true)]
    async fn send_with_retry_recovers_from_429() {
        // First response 429, then 200. Rate limit disabled (interval 0) so the
        // retry exercises the back-off path, not the limiter.
        let t = MockTransport::new();
        t.push_response(429, "slow down");
        t.push_response(200, r#"{"response":{}}"#);
        let client = FleetClient::with_rate_interval(t, Region::Europe, 0).with_token("AT");
        let clock = ManualClock::new(0);
        let resp = send_with_retry(
            &client,
            &EnergyEndpoint::LiveStatus(1),
            &clock,
            Backoff::new(10, 1_000),
            3,
        )
        .await
        .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(client.transport().request_count(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn send_with_retry_gives_up_after_max() {
        let t = MockTransport::new().route("/live_status", 503, "down");
        let client = FleetClient::with_rate_interval(t, Region::Europe, 0).with_token("AT");
        let clock = ManualClock::new(0);
        let err = send_with_retry(
            &client,
            &EnergyEndpoint::LiveStatus(1),
            &clock,
            Backoff::new(10, 1_000),
            2,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, TeslaError::Http { status: 503, .. }));
        // initial try + 2 retries
        assert_eq!(client.transport().request_count(), 3);
    }
}
