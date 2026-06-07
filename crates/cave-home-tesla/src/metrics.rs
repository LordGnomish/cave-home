// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Observability — the Prometheus metrics for the Tesla energy adapter.
//!
//! [`Metrics`] is a tiny in-process registry: the household's live power
//! gauges, plus per-endpoint request-duration and error counters. It renders to
//! the Prometheus text exposition format directly (no client library), matching
//! the convention used elsewhere in cave-home.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use parking_lot::Mutex;

use crate::models::PowerFlowData;

#[derive(Debug, Default)]
struct Inner {
    pv_watts: f64,
    soc_percent: f64,
    grid_import_watts: f64,
    grid_export_watts: f64,
    // endpoint -> (count, sum_secs)
    requests: BTreeMap<String, (u64, f64)>,
    // (endpoint, status) -> count
    errors: BTreeMap<(String, u16), u64>,
}

/// The Tesla energy adapter's metric registry.
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

    /// Update the live power gauges from a power-flow snapshot.
    pub fn record_power_flow(&self, flow: &PowerFlowData) {
        let mut i = self.inner.lock();
        i.pv_watts = flow.pv_watts;
        i.soc_percent = flow.soc_percent;
        i.grid_import_watts = flow.grid_import_watts();
        i.grid_export_watts = flow.grid_export_watts();
    }

    /// Record one completed API request and its wall-clock duration.
    pub fn record_request(&self, endpoint: &str, duration_secs: f64) {
        let mut i = self.inner.lock();
        let entry = i.requests.entry(endpoint.to_string()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += duration_secs;
    }

    /// Record one API error for `endpoint` at HTTP `status`.
    pub fn record_error(&self, endpoint: &str, status: u16) {
        let mut i = self.inner.lock();
        *i.errors.entry((endpoint.to_string(), status)).or_insert(0) += 1;
    }

    /// Render the registry as Prometheus text exposition.
    #[must_use]
    pub fn render(&self) -> String {
        // Snapshot under the lock, then release it before formatting.
        let (pv, soc, gi, ge, requests, errors) = {
            let i = self.inner.lock();
            (
                i.pv_watts,
                i.soc_percent,
                i.grid_import_watts,
                i.grid_export_watts,
                i.requests.clone(),
                i.errors.clone(),
            )
        };
        let mut out = String::new();

        for (name, help, value) in [
            (
                "tesla_pv_power_watts",
                "Instantaneous solar production, watts",
                pv,
            ),
            (
                "tesla_battery_soc_percent",
                "Home battery state of charge, percent",
                soc,
            ),
            (
                "tesla_grid_import_watts",
                "Power drawn from the grid, watts",
                gi,
            ),
            (
                "tesla_grid_export_watts",
                "Power exported to the grid, watts",
                ge,
            ),
        ] {
            let _ = writeln!(out, "# HELP {name} {help}");
            let _ = writeln!(out, "# TYPE {name} gauge");
            let _ = writeln!(out, "{name} {}", fmt_f64(value));
        }

        let dur = "tesla_api_request_duration_seconds";
        let _ = writeln!(out, "# HELP {dur} Fleet API request wall-clock duration");
        let _ = writeln!(out, "# TYPE {dur} summary");
        for (endpoint, (count, sum)) in &requests {
            let _ = writeln!(out, "{dur}_count{{endpoint=\"{endpoint}\"}} {count}");
            let _ = writeln!(out, "{dur}_sum{{endpoint=\"{endpoint}\"}} {}", fmt_f64(*sum));
        }

        let errs = "tesla_api_errors_total";
        let _ = writeln!(out, "# HELP {errs} Fleet API error responses by endpoint and status");
        let _ = writeln!(out, "# TYPE {errs} counter");
        for ((endpoint, status), count) in &errors {
            let _ = writeln!(
                out,
                "{errs}{{endpoint=\"{endpoint}\",status=\"{status}\"}} {count}"
            );
        }

        out
    }
}

/// Format a float without a trailing `.0`, so integers render as `82` not
/// `82.0` (Prometheus accepts both, but this matches the gauge convention).
#[allow(clippy::cast_possible_truncation)] // guarded: integral and < 1e15
fn fmt_f64(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PowerFlowData;

    fn flow() -> PowerFlowData {
        PowerFlowData {
            pv_watts: 3200.0,
            battery_watts: -800.0,
            load_watts: 1500.0,
            grid_watts: -900.0, // exporting 900 W
            soc_percent: 82.0,
        }
    }

    #[test]
    fn power_flow_sets_the_gauges() {
        let m = Metrics::new();
        m.record_power_flow(&flow());
        let out = m.render();
        assert!(out.contains("tesla_pv_power_watts 3200"));
        assert!(out.contains("tesla_battery_soc_percent 82"));
        assert!(out.contains("tesla_grid_export_watts 900"));
        assert!(out.contains("tesla_grid_import_watts 0"));
    }

    #[test]
    fn gauges_have_help_and_type_headers() {
        let m = Metrics::new();
        let out = m.render();
        for name in [
            "tesla_pv_power_watts",
            "tesla_battery_soc_percent",
            "tesla_grid_import_watts",
            "tesla_grid_export_watts",
        ] {
            assert!(out.contains(&format!("# HELP {name} ")), "missing HELP {name}");
            assert!(out.contains(&format!("# TYPE {name} gauge")), "missing TYPE {name}");
        }
    }

    #[test]
    fn request_duration_emits_count_and_sum_per_endpoint() {
        let m = Metrics::new();
        m.record_request("live_status", 0.25);
        m.record_request("live_status", 0.75);
        let out = m.render();
        assert!(out.contains("# TYPE tesla_api_request_duration_seconds summary"));
        assert!(out.contains(r#"tesla_api_request_duration_seconds_count{endpoint="live_status"} 2"#));
        assert!(out.contains(r#"tesla_api_request_duration_seconds_sum{endpoint="live_status"} 1"#));
    }

    #[test]
    fn errors_counter_is_labelled_by_endpoint_and_status() {
        let m = Metrics::new();
        m.record_error("live_status", 500);
        m.record_error("live_status", 500);
        m.record_error("backup", 429);
        let out = m.render();
        assert!(out.contains("# TYPE tesla_api_errors_total counter"));
        assert!(out
            .contains(r#"tesla_api_errors_total{endpoint="live_status",status="500"} 2"#));
        assert!(out.contains(r#"tesla_api_errors_total{endpoint="backup",status="429"} 1"#));
    }

    #[test]
    fn exposition_ends_with_newline() {
        let m = Metrics::new();
        assert!(m.render().ends_with('\n'));
    }
}
