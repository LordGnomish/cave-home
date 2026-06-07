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

use std::collections::BTreeMap;
use std::sync::Mutex;

/// Thread-safe request metrics. Shared (`Arc`) across the server's connection
/// threads.
#[derive(Debug, Default)]
pub struct Metrics {
    /// (verb, code) → count.
    requests: Mutex<BTreeMap<(String, u16), u64>>,
    /// Currently in-flight requests.
    inflight: Mutex<i64>,
}

impl Metrics {
    /// Fresh, zeroed metrics.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the in-flight gauge (call on request entry).
    pub fn inc_inflight(&self) {
        if let Ok(mut g) = self.inflight.lock() {
            *g += 1;
        }
    }

    /// Decrement the in-flight gauge (call on request exit).
    pub fn dec_inflight(&self) {
        if let Ok(mut g) = self.inflight.lock() {
            *g -= 1;
        }
    }

    /// Record one completed request by verb + response code.
    pub fn record_request(&self, verb: &str, code: u16) {
        if let Ok(mut m) = self.requests.lock() {
            *m.entry((verb.to_string(), code)).or_insert(0) += 1;
        }
    }

    /// Read the count for a (verb, code) pair (for tests).
    #[must_use]
    pub fn request_count(&self, verb: &str, code: u16) -> u64 {
        self.requests
            .lock()
            .ok()
            .and_then(|m| m.get(&(verb.to_string(), code)).copied())
            .unwrap_or(0)
    }

    /// Render the metrics in the Prometheus text exposition format.
    #[must_use]
    pub fn to_prometheus(&self) -> String {
        let mut out = String::new();
        out.push_str("# HELP apiserver_request_total Counter of apiserver requests broken out by verb and response code.\n");
        out.push_str("# TYPE apiserver_request_total counter\n");
        if let Ok(m) = self.requests.lock() {
            for ((verb, code), count) in m.iter() {
                out.push_str(&format!(
                    "apiserver_request_total{{verb=\"{verb}\",code=\"{code}\"}} {count}\n"
                ));
            }
        }
        out.push_str("# HELP apiserver_current_inflight_requests Number of requests currently being served.\n");
        out.push_str("# TYPE apiserver_current_inflight_requests gauge\n");
        let inflight = self.inflight.lock().map(|g| *g).unwrap_or(0);
        out.push_str(&format!("apiserver_current_inflight_requests {inflight}\n"));
        out
    }
}

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
