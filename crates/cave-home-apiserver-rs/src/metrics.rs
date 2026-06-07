// SPDX-License-Identifier: Apache-2.0
//! Prometheus instrumentation for the apiserver transport.
//!
//! Behavioural reference: the upstream apiserver metric
//! `apiserver_request_total{verb,code}` (a counter of handled requests) and
//! `apiserver_current_inflight_requests` (a gauge). This is a std-only,
//! dependency-free implementation of the Prometheus text exposition format
//! (`# HELP` / `# TYPE` + samples) — no `prometheus` crate is pulled in. The
//! latency histogram and the full upstream metric set are deferred (see
//! `parity.manifest.toml`).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_counts_by_verb_and_code() {
        let m = Metrics::new();
        m.record_request("create", 201);
        m.record_request("create", 201);
        m.record_request("get", 404);
        assert_eq!(m.request_count("create", 201), 2);
        assert_eq!(m.request_count("get", 404), 1);
        assert_eq!(m.request_count("get", 200), 0);
    }

    #[test]
    fn prometheus_text_has_help_type_and_samples() {
        let m = Metrics::new();
        m.record_request("list", 200);
        let text = m.to_prometheus();
        assert!(text.contains("# HELP apiserver_request_total"));
        assert!(text.contains("# TYPE apiserver_request_total counter"));
        assert!(text.contains("apiserver_request_total{verb=\"list\",code=\"200\"} 1"));
        assert!(text.contains("# TYPE apiserver_current_inflight_requests gauge"));
    }

    #[test]
    fn inflight_gauge_tracks_enter_exit() {
        let m = Metrics::new();
        m.inc_inflight();
        m.inc_inflight();
        m.dec_inflight();
        assert!(m.to_prometheus().contains("apiserver_current_inflight_requests 1"));
    }
}
