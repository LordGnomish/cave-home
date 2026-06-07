// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Observability — the Prometheus metrics for the UniFi console client.
//!
//! [`Metrics`] is a tiny in-process registry the [`crate::client::ConsoleClient`]
//! and the WebSocket engine update as they run: per-endpoint request counts +
//! duration, per-status error counts, login attempts/failures, live-cookie
//! state, and the running WebSocket-event tally per pillar. It renders straight
//! to the Prometheus text exposition format (no client library), matching the
//! convention used across cave-home (see `cave-home-tesla::metrics`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use parking_lot::Mutex;

#[derive(Debug, Default)]
struct Inner {
    // endpoint -> (count, sum_secs)
    requests: BTreeMap<String, (u64, f64)>,
    // status -> count
    errors: BTreeMap<u16, u64>,
    logins: u64,
    login_failures: u64,
    reauths: u64,
    authenticated: bool,
    // pillar (network|access|protect) -> ws events received
    ws_events: BTreeMap<String, u64>,
    ws_reconnects: u64,
}

/// The UniFi console client's metric registry. Cheap to clone-share behind an
/// `Arc`; every method takes `&self`.
#[derive(Debug, Default)]
pub struct Metrics {
    inner: Mutex<Inner>,
}

// The lock-guard lifetimes here are already minimal; the nursery's
// drop-tightening lint just dislikes the idiomatic `let g = lock(); g.field`.
#[allow(clippy::significant_drop_tightening)]
impl Metrics {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one completed API request and its wall-clock duration.
    pub fn record_request(&self, endpoint: &str, duration_secs: f64) {
        let mut i = self.inner.lock();
        let entry = i.requests.entry(endpoint.to_string()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += duration_secs;
    }

    /// Record an API error response by status code.
    pub fn record_error(&self, status: u16) {
        *self.inner.lock().errors.entry(status).or_insert(0) += 1;
    }

    /// Record a successful login and the resulting authenticated state.
    pub fn record_login(&self) {
        let mut i = self.inner.lock();
        i.logins += 1;
        i.authenticated = true;
    }

    /// Record a failed login attempt.
    pub fn record_login_failure(&self) {
        let mut i = self.inner.lock();
        i.login_failures += 1;
        i.authenticated = false;
    }

    /// Record that a request triggered a transparent re-authentication.
    pub fn record_reauth(&self) {
        self.inner.lock().reauths += 1;
    }

    /// Set the live authenticated flag (e.g. cleared on logout).
    pub fn set_authenticated(&self, authenticated: bool) {
        self.inner.lock().authenticated = authenticated;
    }

    /// Record one WebSocket event received for a pillar.
    pub fn record_ws_event(&self, pillar: &str) {
        *self
            .inner
            .lock()
            .ws_events
            .entry(pillar.to_string())
            .or_insert(0) += 1;
    }

    /// Record a WebSocket reconnect.
    pub fn record_ws_reconnect(&self) {
        self.inner.lock().ws_reconnects += 1;
    }

    /// Total requests recorded across all endpoints.
    #[must_use]
    pub fn total_requests(&self) -> u64 {
        self.inner.lock().requests.values().map(|(c, _)| c).sum()
    }

    /// Total WebSocket events recorded across all pillars.
    #[must_use]
    pub fn total_ws_events(&self) -> u64 {
        self.inner.lock().ws_events.values().sum()
    }

    /// Render the registry in the Prometheus text exposition format.
    #[must_use]
    pub fn render_prometheus(&self) -> String {
        let i = self.inner.lock();
        let mut out = String::new();

        let _ = writeln!(
            out,
            "# HELP unifi_requests_total Total UniFi API requests by endpoint."
        );
        let _ = writeln!(out, "# TYPE unifi_requests_total counter");
        for (endpoint, (count, _)) in &i.requests {
            let _ = writeln!(
                out,
                "unifi_requests_total{{endpoint=\"{endpoint}\"}} {count}"
            );
        }

        let _ = writeln!(
            out,
            "# HELP unifi_request_duration_seconds_sum Summed request duration by endpoint."
        );
        let _ = writeln!(out, "# TYPE unifi_request_duration_seconds_sum counter");
        for (endpoint, (_, sum)) in &i.requests {
            let _ = writeln!(
                out,
                "unifi_request_duration_seconds_sum{{endpoint=\"{endpoint}\"}} {sum}"
            );
        }

        let _ = writeln!(
            out,
            "# HELP unifi_errors_total API error responses by HTTP status."
        );
        let _ = writeln!(out, "# TYPE unifi_errors_total counter");
        for (status, count) in &i.errors {
            let _ = writeln!(out, "unifi_errors_total{{status=\"{status}\"}} {count}");
        }

        let _ = writeln!(out, "# HELP unifi_logins_total Successful console logins.");
        let _ = writeln!(out, "# TYPE unifi_logins_total counter");
        let _ = writeln!(out, "unifi_logins_total {}", i.logins);
        let _ = writeln!(
            out,
            "# HELP unifi_login_failures_total Failed console logins."
        );
        let _ = writeln!(out, "# TYPE unifi_login_failures_total counter");
        let _ = writeln!(out, "unifi_login_failures_total {}", i.login_failures);
        let _ = writeln!(
            out,
            "# HELP unifi_reauth_total Transparent re-authentications on 401."
        );
        let _ = writeln!(out, "# TYPE unifi_reauth_total counter");
        let _ = writeln!(out, "unifi_reauth_total {}", i.reauths);

        let _ = writeln!(
            out,
            "# HELP unifi_authenticated Whether the client currently holds a session (1/0)."
        );
        let _ = writeln!(out, "# TYPE unifi_authenticated gauge");
        let _ = writeln!(
            out,
            "unifi_authenticated {}",
            u8::from(i.authenticated)
        );

        let _ = writeln!(
            out,
            "# HELP unifi_ws_events_total Real-time WebSocket events by pillar."
        );
        let _ = writeln!(out, "# TYPE unifi_ws_events_total counter");
        for (pillar, count) in &i.ws_events {
            let _ = writeln!(
                out,
                "unifi_ws_events_total{{pillar=\"{pillar}\"}} {count}"
            );
        }
        let _ = writeln!(
            out,
            "# HELP unifi_ws_reconnects_total WebSocket reconnect attempts."
        );
        let _ = writeln!(out, "# TYPE unifi_ws_reconnects_total counter");
        let _ = writeln!(out, "unifi_ws_reconnects_total {}", i.ws_reconnects);

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_accumulate_per_endpoint() {
        let m = Metrics::new();
        m.record_request("network/clients", 0.10);
        m.record_request("network/clients", 0.20);
        m.record_request("protect/bootstrap", 0.50);
        assert_eq!(m.total_requests(), 3);
        let out = m.render_prometheus();
        assert!(out.contains("unifi_requests_total{endpoint=\"network/clients\"} 2"));
        assert!(out
            .contains("unifi_request_duration_seconds_sum{endpoint=\"network/clients\"} 0.3"));
    }

    #[test]
    fn errors_count_by_status() {
        let m = Metrics::new();
        m.record_error(401);
        m.record_error(401);
        m.record_error(500);
        let out = m.render_prometheus();
        assert!(out.contains("unifi_errors_total{status=\"401\"} 2"));
        assert!(out.contains("unifi_errors_total{status=\"500\"} 1"));
    }

    #[test]
    fn login_and_auth_state_render() {
        let m = Metrics::new();
        m.record_login();
        m.record_login_failure();
        m.record_reauth();
        let out = m.render_prometheus();
        assert!(out.contains("unifi_logins_total 1"));
        assert!(out.contains("unifi_login_failures_total 1"));
        assert!(out.contains("unifi_reauth_total 1"));
        // record_login_failure set authenticated=false last.
        assert!(out.contains("unifi_authenticated 0"));
        m.set_authenticated(true);
        assert!(m.render_prometheus().contains("unifi_authenticated 1"));
    }

    #[test]
    fn ws_events_tally_per_pillar() {
        let m = Metrics::new();
        m.record_ws_event("access");
        m.record_ws_event("access");
        m.record_ws_event("protect");
        m.record_ws_reconnect();
        assert_eq!(m.total_ws_events(), 3);
        let out = m.render_prometheus();
        assert!(out.contains("unifi_ws_events_total{pillar=\"access\"} 2"));
        assert!(out.contains("unifi_ws_events_total{pillar=\"protect\"} 1"));
        assert!(out.contains("unifi_ws_reconnects_total 1"));
    }

    #[test]
    fn exposition_has_help_and_type_lines() {
        let m = Metrics::new();
        m.record_request("x", 0.0);
        let out = m.render_prometheus();
        assert!(out.contains("# HELP unifi_requests_total"));
        assert!(out.contains("# TYPE unifi_requests_total counter"));
    }
}
