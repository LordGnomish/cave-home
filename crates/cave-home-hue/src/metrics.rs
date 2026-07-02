// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Observability — Prometheus metrics for the Hue bridge client.
//!
//! [`Metrics`] is a tiny in-process registry: live lighting gauges (lights on /
//! total, bridge reachability, scene count), an `EventStream` delivery counter,
//! and per-endpoint request-duration + error counters. It renders straight to
//! the Prometheus text exposition format (no client library), matching the
//! convention used by the Tesla adapter and elsewhere in cave-home.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use parking_lot::Mutex;

#[derive(Debug, Default)]
struct Inner {
    lights_on: u32,
    lights_total: u32,
    scenes_total: u32,
    bridge_reachable: bool,
    events_total: u64,
    // endpoint -> (count, sum_secs)
    requests: BTreeMap<String, (u64, f64)>,
    // (endpoint, status) -> count
    errors: BTreeMap<(String, u16), u64>,
}

/// The Hue bridge client's metric registry.
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

    /// Update the lighting gauges from a freshly-synced snapshot.
    pub fn record_lights(&self, on: u32, total: u32) {
        let mut i = self.inner.lock();
        i.lights_on = on;
        i.lights_total = total;
    }

    /// Record the total number of scenes known to the bridge.
    pub fn record_scenes(&self, total: u32) {
        self.inner.lock().scenes_total = total;
    }

    /// Flag whether the bridge is currently reachable.
    pub fn set_bridge_reachable(&self, reachable: bool) {
        self.inner.lock().bridge_reachable = reachable;
    }

    /// Count one `EventStream` event delivered to the controllers.
    pub fn record_event(&self) {
        self.inner.lock().events_total += 1;
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
        let (on, total, scenes, reachable, events, requests, errors) = {
            let i = self.inner.lock();
            (
                i.lights_on,
                i.lights_total,
                i.scenes_total,
                i.bridge_reachable,
                i.events_total,
                i.requests.clone(),
                i.errors.clone(),
            )
        };
        let mut out = String::new();

        for (name, help, value) in [
            ("hue_lights_on", "Hue lights currently switched on", f64::from(on)),
            ("hue_lights_total", "Hue lights known to the bridge", f64::from(total)),
            ("hue_scenes_total", "Hue scenes known to the bridge", f64::from(scenes)),
            (
                "hue_bridge_reachable",
                "Whether the Hue bridge is reachable (1) or not (0)",
                if reachable { 1.0 } else { 0.0 },
            ),
        ] {
            let _ = writeln!(out, "# HELP {name} {help}");
            let _ = writeln!(out, "# TYPE {name} gauge");
            let _ = writeln!(out, "{name} {}", fmt_f64(value));
        }

        let evt = "hue_eventstream_events_total";
        let _ = writeln!(out, "# HELP {evt} EventStream events delivered to controllers");
        let _ = writeln!(out, "# TYPE {evt} counter");
        let _ = writeln!(out, "{evt} {events}");

        let dur = "hue_api_request_duration_seconds";
        let _ = writeln!(out, "# HELP {dur} CLIP API request wall-clock duration");
        let _ = writeln!(out, "# TYPE {dur} summary");
        for (endpoint, (count, sum)) in &requests {
            let _ = writeln!(out, "{dur}_count{{endpoint=\"{endpoint}\"}} {count}");
            let _ = writeln!(out, "{dur}_sum{{endpoint=\"{endpoint}\"}} {}", fmt_f64(*sum));
        }

        let errs = "hue_api_errors_total";
        let _ = writeln!(out, "# HELP {errs} CLIP API error responses by endpoint and status");
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

/// Format a float without a trailing `.0`, so integers render as `8` not `8.0`
/// (Prometheus accepts both, but this matches the gauge convention).
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

    #[test]
    fn lighting_gauges_render_with_help_and_type() {
        let m = Metrics::new();
        m.record_lights(3, 8);
        m.record_scenes(5);
        m.set_bridge_reachable(true);
        let out = m.render();
        assert!(out.contains("hue_lights_on 3"));
        assert!(out.contains("hue_lights_total 8"));
        assert!(out.contains("hue_scenes_total 5"));
        assert!(out.contains("hue_bridge_reachable 1"));
        for name in ["hue_lights_on", "hue_lights_total", "hue_bridge_reachable"] {
            assert!(out.contains(&format!("# TYPE {name} gauge")), "missing TYPE {name}");
        }
    }

    #[test]
    fn unreachable_bridge_renders_zero() {
        let m = Metrics::new();
        m.set_bridge_reachable(false);
        assert!(m.render().contains("hue_bridge_reachable 0"));
    }

    #[test]
    fn event_counter_accumulates() {
        let m = Metrics::new();
        m.record_event();
        m.record_event();
        m.record_event();
        let out = m.render();
        assert!(out.contains("# TYPE hue_eventstream_events_total counter"));
        assert!(out.contains("hue_eventstream_events_total 3"));
    }

    #[test]
    fn request_duration_emits_count_and_sum_per_endpoint() {
        let m = Metrics::new();
        m.record_request("get_lights", 0.25);
        m.record_request("get_lights", 0.75);
        let out = m.render();
        assert!(out.contains("# TYPE hue_api_request_duration_seconds summary"));
        assert!(out.contains(r#"hue_api_request_duration_seconds_count{endpoint="get_lights"} 2"#));
        assert!(out.contains(r#"hue_api_request_duration_seconds_sum{endpoint="get_lights"} 1"#));
    }

    #[test]
    fn errors_counter_is_labelled_by_endpoint_and_status() {
        let m = Metrics::new();
        m.record_error("set_light", 403);
        m.record_error("set_light", 403);
        m.record_error("recall_scene", 503);
        let out = m.render();
        assert!(out.contains("# TYPE hue_api_errors_total counter"));
        assert!(out.contains(r#"hue_api_errors_total{endpoint="set_light",status="403"} 2"#));
        assert!(out.contains(r#"hue_api_errors_total{endpoint="recall_scene",status="503"} 1"#));
    }

    #[test]
    fn exposition_ends_with_newline() {
        let m = Metrics::new();
        assert!(m.render().ends_with('\n'));
    }
}
