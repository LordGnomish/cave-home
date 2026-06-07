// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Observability — the Prometheus metrics for the kine datastore.
//!
//! [`KineMetrics`] is a tiny, dependency-free in-process registry: per-operation
//! request counts / durations / errors, the live backend connection gauge, and
//! the compaction counters the audit named (`kine_request_duration`,
//! `kine_db_connections`, `kine_compaction_runs`). It renders straight to the
//! Prometheus text exposition format — no client library — matching the
//! convention used elsewhere in cave-home (e.g. the Tesla adapter).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Mutex;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_exposes_all_named_metric_families() {
        let m = KineMetrics::new();
        let out = m.render();
        for name in [
            "kine_request_duration_seconds",
            "kine_request_total",
            "kine_request_errors_total",
            "kine_db_connections",
            "kine_compaction_runs_total",
        ] {
            assert!(out.contains(&format!("# TYPE {name}")), "missing TYPE for {name}");
            assert!(out.contains(&format!("# HELP {name}")), "missing HELP for {name}");
        }
    }

    #[test]
    fn record_request_accumulates_count_and_duration_sum() {
        let m = KineMetrics::new();
        m.record_request("put", 0.002, true);
        m.record_request("put", 0.003, true);
        let out = m.render();
        assert!(out.contains("kine_request_total{operation=\"put\"} 2"));
        assert!(out.contains("kine_request_duration_seconds_count{operation=\"put\"} 2"));
        assert!(out.contains("kine_request_duration_seconds_sum{operation=\"put\"} 0.005"));
    }

    #[test]
    fn failed_requests_increment_the_error_counter() {
        let m = KineMetrics::new();
        m.record_request("range", 0.001, true);
        m.record_request("range", 0.001, false);
        let out = m.render();
        assert!(out.contains("kine_request_total{operation=\"range\"} 2"));
        assert!(out.contains("kine_request_errors_total{operation=\"range\"} 1"));
    }

    #[test]
    fn operations_render_as_separate_label_series() {
        let m = KineMetrics::new();
        m.record_request("put", 0.001, true);
        m.record_request("delete", 0.001, true);
        let out = m.render();
        assert!(out.contains("kine_request_total{operation=\"put\"} 1"));
        assert!(out.contains("kine_request_total{operation=\"delete\"} 1"));
    }

    #[test]
    fn db_connections_is_a_settable_gauge() {
        let m = KineMetrics::new();
        m.set_db_connections(3);
        assert!(m.render().contains("kine_db_connections 3"));
        m.set_db_connections(1);
        assert!(m.render().contains("kine_db_connections 1"));
    }

    #[test]
    fn compaction_counters_accumulate_runs_and_rows() {
        let m = KineMetrics::new();
        m.record_compaction(10);
        m.record_compaction(5);
        let out = m.render();
        assert!(out.contains("kine_compaction_runs_total 2"));
        assert!(out.contains("kine_compaction_rows_removed_total 15"));
    }

    #[test]
    fn render_is_well_formed_exposition_text() {
        let m = KineMetrics::new();
        m.record_request("put", 0.001, true);
        let out = m.render();
        // every non-comment, non-empty line is `name value` or `name{..} value`
        for line in out.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            assert!(line.rsplit_once(' ').is_some(), "malformed sample line: {line}");
        }
    }
}
