// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Prometheus metrics for the free@home client.
//!
//! A small lock-free registry of atomic counters and a gauge, rendered to the
//! Prometheus text exposition format. Kept dependency-free on purpose: the hub
//! exposes a single `/metrics` endpoint and concatenates each integration's
//! [`Metrics::render`] output.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

/// All counters/gauges this integration exports.
#[derive(Debug, Default)]
pub struct Metrics {
    state_changes: AtomicU64,
    api_requests: AtomicU64,
    api_errors: AtomicU64,
    auth_failures: AtomicU64,
    ws_reconnects: AtomicU64,
    api_latency_ms_sum: AtomicU64,
    api_latency_count: AtomicU64,
    connected: AtomicU64,
}

impl Metrics {
    /// A fresh, all-zero registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// One datapoint state change was received.
    pub fn inc_state_changes(&self) {
        self.state_changes.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a completed REST request with its latency.
    pub fn observe_latency_ms(&self, ms: u64) {
        self.api_requests.fetch_add(1, Ordering::Relaxed);
        self.api_latency_ms_sum.fetch_add(ms, Ordering::Relaxed);
        self.api_latency_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a failed REST request.
    pub fn record_error(&self) {
        self.api_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an authentication failure (HTTP 401/403).
    pub fn record_auth_failure(&self) {
        self.auth_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a WebSocket reconnect attempt.
    pub fn record_reconnect(&self) {
        self.ws_reconnects.fetch_add(1, Ordering::Relaxed);
    }

    /// Set the connected gauge (true → 1, false → 0).
    pub fn set_connected(&self, connected: bool) {
        self.connected
            .store(u64::from(connected), Ordering::Relaxed);
    }

    /// The number of state changes observed.
    pub fn state_changes(&self) -> u64 {
        self.state_changes.load(Ordering::Relaxed)
    }

    /// The number of latency observations recorded.
    pub fn latency_count(&self) -> u64 {
        self.api_latency_count.load(Ordering::Relaxed)
    }

    /// Render every metric in Prometheus text exposition format.
    pub fn render(&self) -> String {
        let load = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let mut out = String::new();
        let counter = |out: &mut String, name: &str, help: &str, value: u64| {
            // Writing to a String is infallible.
            let _ = writeln!(out, "# HELP {name} {help}");
            let _ = writeln!(out, "# TYPE {name} counter");
            let _ = writeln!(out, "{name} {value}");
        };
        counter(
            &mut out,
            "freeathome_device_state_changes_total",
            "Datapoint state changes received from the SysAP.",
            load(&self.state_changes),
        );
        counter(
            &mut out,
            "freeathome_api_requests_total",
            "REST requests issued to the SysAP.",
            load(&self.api_requests),
        );
        counter(
            &mut out,
            "freeathome_api_errors_total",
            "REST requests that failed.",
            load(&self.api_errors),
        );
        counter(
            &mut out,
            "freeathome_auth_failures_total",
            "Authentication failures (HTTP 401/403).",
            load(&self.auth_failures),
        );
        counter(
            &mut out,
            "freeathome_ws_reconnects_total",
            "WebSocket reconnect attempts.",
            load(&self.ws_reconnects),
        );
        counter(
            &mut out,
            "freeathome_api_request_duration_ms_sum",
            "Cumulative REST request latency in milliseconds.",
            load(&self.api_latency_ms_sum),
        );
        counter(
            &mut out,
            "freeathome_api_request_duration_ms_count",
            "Number of REST latency observations.",
            load(&self.api_latency_count),
        );
        out.push_str("# HELP freeathome_connected Whether the WebSocket is currently connected.\n");
        out.push_str("# TYPE freeathome_connected gauge\n");
        let _ = writeln!(out, "freeathome_connected {}", load(&self.connected));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_increment() {
        let m = Metrics::new();
        m.inc_state_changes();
        m.inc_state_changes();
        assert_eq!(m.state_changes(), 2);
    }

    #[test]
    fn render_contains_counter_value_and_type() {
        let m = Metrics::new();
        m.inc_state_changes();
        let out = m.render();
        assert!(out.contains("freeathome_device_state_changes_total 1"));
        assert!(out.contains("# TYPE freeathome_device_state_changes_total counter"));
    }

    #[test]
    fn observe_latency_tracks_sum_and_count() {
        let m = Metrics::new();
        m.observe_latency_ms(10);
        m.observe_latency_ms(30);
        assert_eq!(m.latency_count(), 2);
        let out = m.render();
        assert!(out.contains("freeathome_api_request_duration_ms_sum 40"));
        assert!(out.contains("freeathome_api_requests_total 2"));
    }

    #[test]
    fn connected_gauge_renders_zero_or_one() {
        let m = Metrics::new();
        m.set_connected(true);
        assert!(m.render().contains("freeathome_connected 1"));
        m.set_connected(false);
        assert!(m.render().contains("freeathome_connected 0"));
    }

    #[test]
    fn auth_failures_counter() {
        let m = Metrics::new();
        m.record_auth_failure();
        assert!(m.render().contains("freeathome_auth_failures_total 1"));
    }

    #[test]
    fn reconnect_counter() {
        let m = Metrics::new();
        m.record_reconnect();
        m.record_reconnect();
        assert!(m.render().contains("freeathome_ws_reconnects_total 2"));
    }

    #[test]
    fn api_errors_counter_and_help_lines() {
        let m = Metrics::new();
        m.record_error();
        let out = m.render();
        assert!(out.contains("freeathome_api_errors_total 1"));
        assert!(out.contains("# HELP freeathome_api_errors_total"));
    }
}
