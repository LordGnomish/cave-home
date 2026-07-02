// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! Comparing two snapshots to surface day-over-day deltas.

use crate::snapshot::{Snapshot, SubsystemMetric};

/// Per-subsystem change between two snapshots.
#[derive(Debug, Clone, PartialEq)]
pub struct SubsystemDelta {
    /// Subsystem name.
    pub name: String,
    /// Rollup group.
    pub group: String,
    /// Change in upstream LOC.
    pub d_upstream_loc: i64,
    /// Change in port LOC.
    pub d_port_loc: i64,
    /// Change in honest real-%.
    pub d_real_pct: f64,
    /// Change in passing tests.
    pub d_tests_passed: i64,
    /// Change in stub count.
    pub d_stubs: i64,
    /// `true` if this subsystem did not exist in the previous snapshot.
    pub is_new: bool,
}

/// Day-over-day diff of two whole snapshots.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotDiff {
    /// Previous snapshot date (empty if there was no baseline).
    pub from_date: String,
    /// Current snapshot date.
    pub to_date: String,
    /// Per-subsystem deltas, in current-snapshot order.
    pub subsystems: Vec<SubsystemDelta>,
    /// Change in overall weighted real-%.
    pub d_overall_real_pct: f64,
}

#[allow(clippy::cast_possible_wrap)]
const fn d(cur: u64, prev: u64) -> i64 {
    cur as i64 - prev as i64
}

/// Compute the delta from `prev` to `cur`. When `prev` is `None`, every
/// subsystem is reported as new and deltas equal current values.
#[must_use]
#[allow(clippy::option_if_let_else)] // the explicit match reads clearer here
pub fn diff(prev: Option<&Snapshot>, cur: &Snapshot) -> SnapshotDiff {
    let lookup = |name: &str| -> Option<&SubsystemMetric> { prev.and_then(|p| p.subsystem(name)) };
    let subsystems = cur
        .subsystems
        .iter()
        .map(|m| match lookup(&m.name) {
            Some(p) => SubsystemDelta {
                name: m.name.clone(),
                group: m.group.clone(),
                d_upstream_loc: d(m.upstream_loc, p.upstream_loc),
                d_port_loc: d(m.port_loc, p.port_loc),
                d_real_pct: m.real_pct - p.real_pct,
                d_tests_passed: d(m.tests_passed, p.tests_passed),
                d_stubs: d(m.stubs.total(), p.stubs.total()),
                is_new: false,
            },
            None => SubsystemDelta {
                name: m.name.clone(),
                group: m.group.clone(),
                d_upstream_loc: d(m.upstream_loc, 0),
                d_port_loc: d(m.port_loc, 0),
                d_real_pct: m.real_pct,
                d_tests_passed: d(m.tests_passed, 0),
                d_stubs: d(m.stubs.total(), 0),
                is_new: true,
            },
        })
        .collect();

    let prev_overall = prev.map_or(0.0, Snapshot::overall_real_pct);
    SnapshotDiff {
        from_date: prev.map(|p| p.date.clone()).unwrap_or_default(),
        to_date: cur.date.clone(),
        subsystems,
        d_overall_real_pct: cur.overall_real_pct() - prev_overall,
    }
}

impl SnapshotDiff {
    /// Look up a subsystem's delta by name.
    #[must_use]
    pub fn subsystem(&self, name: &str) -> Option<&SubsystemDelta> {
        self.subsystems.iter().find(|d| d.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stubs::StubCount;

    fn snap(date: &str, metrics: Vec<SubsystemMetric>) -> Snapshot {
        Snapshot {
            project: "cave-home".into(),
            date: date.into(),
            generated_at: format!("{date}T06:00:00Z"),
            subsystems: metrics,
        }
    }

    /// Build a metric; `has_upstream` is inferred from `upstream > 0`.
    fn dm(
        name: &str,
        upstream: u64,
        port: u64,
        passed: u64,
        failed: u64,
        stubs: StubCount,
    ) -> SubsystemMetric {
        SubsystemMetric::derive(
            name,
            "k3s",
            upstream,
            port,
            upstream > 0,
            passed,
            failed,
            0,
            stubs,
        )
    }

    #[test]
    fn diff_reports_growth() {
        let prev = snap(
            "2026-06-06",
            vec![dm("kine", 1000, 200, 5, 0, StubCount::default())],
        );
        let cur = snap(
            "2026-06-07",
            vec![dm("kine", 1000, 400, 9, 0, StubCount::default())],
        );
        let dd = diff(Some(&prev), &cur);
        let k = dd.subsystem("kine").unwrap();
        assert!(!k.is_new);
        assert_eq!(k.d_port_loc, 200);
        assert_eq!(k.d_tests_passed, 4);
        assert!(k.d_real_pct > 0.0);
        assert!(dd.d_overall_real_pct > 0.0);
        assert_eq!(dd.from_date, "2026-06-06");
    }

    #[test]
    fn new_subsystem_flagged() {
        let prev = snap("2026-06-06", vec![]);
        let cur = snap(
            "2026-06-07",
            vec![dm("hue", 0, 100, 3, 0, StubCount::default())],
        );
        let dd = diff(Some(&prev), &cur);
        assert!(dd.subsystem("hue").unwrap().is_new);
    }

    #[test]
    fn no_baseline_treats_all_new() {
        let cur = snap(
            "2026-06-07",
            vec![dm("hue", 0, 100, 3, 0, StubCount::default())],
        );
        let dd = diff(None, &cur);
        assert_eq!(dd.from_date, "");
        assert!(dd.subsystem("hue").unwrap().is_new);
    }

    #[test]
    fn detects_regressions() {
        let prev = snap(
            "2026-06-06",
            vec![dm("kine", 1000, 400, 9, 0, StubCount::default())],
        );
        let cur = snap(
            "2026-06-07",
            vec![dm(
                "kine",
                1000,
                400,
                7,
                2,
                StubCount {
                    panic: 1,
                    ..StubCount::default()
                },
            )],
        );
        let dd = diff(Some(&prev), &cur);
        let k = dd.subsystem("kine").unwrap();
        assert_eq!(k.d_tests_passed, -2);
        assert_eq!(k.d_stubs, 1);
        assert!(k.d_real_pct < 0.0);
    }
}
