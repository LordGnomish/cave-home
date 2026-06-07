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

/// Per-operation tallies: `(count, error_count, duration_sum_secs)`.
#[derive(Debug, Clone, Copy, Default)]
struct OpStat {
    count: u64,
    errors: u64,
    duration_secs: f64,
}

#[derive(Debug, Default)]
struct Inner {
    /// operation name -> tallies
    ops: BTreeMap<String, OpStat>,
    db_connections: u64,
    compaction_runs: u64,
    compaction_rows_removed: u64,
}

/// The kine datastore's metric registry. Cheap to share behind an `Arc`; every
/// method takes `&self`.
#[derive(Debug, Default)]
pub struct KineMetrics {
    inner: Mutex<Inner>,
}

// The guard lifetimes here are already minimal; the nursery drop-tightening
// lint just dislikes the idiomatic `let g = lock(); g.field` shape.
#[allow(clippy::significant_drop_tightening)]
impl KineMetrics {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one completed backend operation, its wall-clock duration, and
    /// whether it succeeded. `operation` is a low-cardinality verb
    /// (`range` / `put` / `delete` / `txn` / `compact` / `watch`).
    pub fn record_request(&self, operation: &str, duration_secs: f64, ok: bool) {
        let Ok(mut inner) = self.inner.lock() else { return };
        let stat = inner.ops.entry(operation.to_string()).or_default();
        stat.count += 1;
        stat.duration_secs += duration_secs;
        if !ok {
            stat.errors += 1;
        }
    }

    /// Set the live open-backend-connections gauge.
    pub fn set_db_connections(&self, n: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.db_connections = n;
        }
    }

    /// Record one compaction run that removed `rows_removed` rows.
    pub fn record_compaction(&self, rows_removed: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.compaction_runs += 1;
            inner.compaction_rows_removed += rows_removed;
        }
    }

    /// Render the registry as Prometheus text exposition.
    #[must_use]
    pub fn render(&self) -> String {
        let Ok(inner) = self.inner.lock() else { return String::new() };
        let mut out = String::new();

        out.push_str("# HELP kine_request_duration_seconds Time spent serving each kine backend operation.\n");
        out.push_str("# TYPE kine_request_duration_seconds summary\n");
        for (op, stat) in &inner.ops {
            let _ = writeln!(
                out,
                "kine_request_duration_seconds_count{{operation=\"{op}\"}} {}",
                stat.count
            );
            let _ = writeln!(
                out,
                "kine_request_duration_seconds_sum{{operation=\"{op}\"}} {}",
                fmt_f64(stat.duration_secs)
            );
        }

        out.push_str("# HELP kine_request_total Total kine backend operations.\n");
        out.push_str("# TYPE kine_request_total counter\n");
        for (op, stat) in &inner.ops {
            let _ = writeln!(out, "kine_request_total{{operation=\"{op}\"}} {}", stat.count);
        }

        out.push_str("# HELP kine_request_errors_total Failed kine backend operations.\n");
        out.push_str("# TYPE kine_request_errors_total counter\n");
        for (op, stat) in &inner.ops {
            let _ = writeln!(out, "kine_request_errors_total{{operation=\"{op}\"}} {}", stat.errors);
        }

        out.push_str("# HELP kine_db_connections Open backend database connections.\n");
        out.push_str("# TYPE kine_db_connections gauge\n");
        let _ = writeln!(out, "kine_db_connections {}", inner.db_connections);

        out.push_str("# HELP kine_compaction_runs_total Total compaction runs.\n");
        out.push_str("# TYPE kine_compaction_runs_total counter\n");
        let _ = writeln!(out, "kine_compaction_runs_total {}", inner.compaction_runs);

        out.push_str("# HELP kine_compaction_rows_removed_total Rows removed by compaction.\n");
        out.push_str("# TYPE kine_compaction_rows_removed_total counter\n");
        let _ = writeln!(out, "kine_compaction_rows_removed_total {}", inner.compaction_rows_removed);

        out
    }
}

/// Format an `f64` for exposition at fixed precision (avoiding binary-float
/// noise like `0.005000000000000001`), then trim trailing zeros and any bare
/// trailing dot so whole seconds render as `1`, not `1.000000000`.
fn fmt_f64(v: f64) -> String {
    let s = format!("{v:.9}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() }
}

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
