// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Prometheus metrics for the free@home client.

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
