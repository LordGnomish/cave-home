// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! Rendering the daily markdown progress report.
//!
//! The report groups subsystems by their rollup group, shows a table per group
//! (upstream LOC, port LOC, ratio, tests, stubs, real-% and the day-over-day
//! delta), prints aggregate completion for K3s and Smart-Home, and ends with a
//! 30-day text trend chart of overall completion.

use std::fmt::Write as _;

use crate::diff::SnapshotDiff;
use crate::snapshot::Snapshot;

/// Unicode sparkline ramp, low to high.
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render a sparkline for `values` scaled to a fixed `[0, 100]` domain, so the
/// bars are comparable across days rather than auto-scaled.
#[must_use]
pub fn sparkline(values: &[f64]) -> String {
    values
        .iter()
        .map(|v| {
            let clamped = v.clamp(0.0, 100.0);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let idx = ((clamped / 100.0) * (SPARK.len() as f64 - 1.0)).round() as usize;
            SPARK[idx.min(SPARK.len() - 1)]
        })
        .collect()
}

/// Format a signed delta with an explicit sign and a fixed precision.
fn signed(v: f64) -> String {
    if v.abs() < 0.05 {
        "·".to_owned()
    } else {
        format!("{v:+.1}")
    }
}

#[allow(clippy::cast_precision_loss)]
fn ratio_pct(m: &crate::snapshot::SubsystemMetric) -> f64 {
    m.ratio * 100.0
}

/// Render the full daily report.
///
/// * `cur` — today's snapshot.
/// * `dd` — the diff against the previous snapshot (use [`crate::diff::diff`]).
/// * `history` — all snapshots (ascending by date) for the trend chart.
#[must_use]
pub fn render_markdown(cur: &Snapshot, dd: &SnapshotDiff, history: &[Snapshot]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {} — daily progress · {}", cur.project, cur.date);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "_Generated {} by cave-home-tracker._",
        cur.generated_at
    );
    let _ = writeln!(out);

    // Aggregates.
    let k3s = cur.group_real_pct("k3s");
    let smart = cur.complement_real_pct("k3s");
    let overall = cur.overall_real_pct();
    let _ = writeln!(out, "## Aggregate completion (honest)");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Rollup | Real % | Δ vs prev |");
    let _ = writeln!(out, "|---|---:|---:|");
    let _ = writeln!(
        out,
        "| **Overall** | {overall:.1}% | {} |",
        signed(dd.d_overall_real_pct)
    );
    let _ = writeln!(out, "| K3s | {k3s:.1}% | — |");
    let _ = writeln!(out, "| Smart-Home | {smart:.1}% | — |");
    let _ = writeln!(out);

    // One table per group.
    for group in cur.groups() {
        let _ = writeln!(out, "## {group}");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Subsystem | Upstream LOC | Port LOC | Ratio | Tests (P/F/I) | Stubs | Real % | Δ Real % |"
        );
        let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---:|");
        for m in cur.subsystems.iter().filter(|m| m.group == group) {
            let delta = dd.subsystem(&m.name).map_or_else(
                || "—".to_owned(),
                |d| {
                    if d.is_new {
                        "new".to_owned()
                    } else {
                        signed(d.d_real_pct)
                    }
                },
            );
            let _ = writeln!(
                out,
                "| {} | {} | {} | {:.0}% | {}/{}/{} | {} | {:.1}% | {} |",
                m.name,
                m.upstream_loc,
                m.port_loc,
                ratio_pct(m),
                m.tests_passed,
                m.tests_failed,
                m.tests_ignored,
                m.stubs.total(),
                m.real_pct,
                delta,
            );
        }
        let _ = writeln!(
            out,
            "| **{group} aggregate** | | | | | | **{:.1}%** | |",
            cur.group_real_pct(&group)
        );
        let _ = writeln!(out);
    }

    // 30-day trend.
    let _ = writeln!(out, "## Overall trend (last 30 snapshots)");
    let _ = writeln!(out);
    let recent: Vec<&Snapshot> = history.iter().rev().take(30).rev().collect();
    if recent.is_empty() {
        let _ = writeln!(out, "_No history yet._");
    } else {
        let values: Vec<f64> = recent.iter().map(|s| s.overall_real_pct()).collect();
        let _ = writeln!(out, "```");
        let _ = writeln!(out, "{}", sparkline(&values));
        if let (Some(first), Some(last)) = (recent.first(), recent.last()) {
            let _ = writeln!(
                out,
                "{} {:.1}%  →  {} {:.1}%",
                first.date,
                first.overall_real_pct(),
                last.date,
                last.overall_real_pct(),
            );
        }
        let _ = writeln!(out, "```");
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "> Real % = coverage × test-pass-rate × stub-integrity. No paperwork."
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff;
    use crate::snapshot::SubsystemMetric;
    use crate::stubs::StubCount;

    fn snap(date: &str, real_kine: f64) -> Snapshot {
        Snapshot {
            project: "cave-home".into(),
            date: date.into(),
            generated_at: format!("{date}T06:00:00Z"),
            subsystems: vec![
                SubsystemMetric {
                    real_pct: real_kine,
                    ..SubsystemMetric::derive(
                        "kine",
                        "k3s",
                        1000,
                        500,
                        true,
                        8,
                        0,
                        1,
                        StubCount::default(),
                    )
                },
                SubsystemMetric::derive(
                    "hue",
                    "smart-home",
                    0,
                    200,
                    false,
                    5,
                    0,
                    0,
                    StubCount::default(),
                ),
            ],
        }
    }

    #[test]
    fn sparkline_scales_fixed_domain() {
        let s = sparkline(&[0.0, 50.0, 100.0]);
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars[0], '▁');
        assert_eq!(chars[2], '█');
        assert_eq!(chars.len(), 3);
    }

    #[test]
    fn report_has_tables_aggregates_and_trend() {
        let prev = snap("2026-06-06", 30.0);
        let cur = snap("2026-06-07", 40.0);
        let history = vec![prev.clone(), cur.clone()];
        let dd = diff::diff(Some(&prev), &cur);
        let md = render_markdown(&cur, &dd, &history);

        assert!(md.contains("# cave-home — daily progress · 2026-06-07"));
        assert!(md.contains("## Aggregate completion (honest)"));
        assert!(md.contains("## k3s"));
        assert!(md.contains("## smart-home"));
        assert!(md.contains("| kine |"));
        assert!(md.contains("Overall trend"));
        assert!(md.contains("```"));
        // delta column present for kine
        assert!(md.contains("k3s aggregate"));
    }

    #[test]
    fn new_subsystem_shows_new_in_delta() {
        let cur = snap("2026-06-07", 40.0);
        let dd = diff::diff(None, &cur);
        let md = render_markdown(&cur, &dd, &[cur.clone()]);
        assert!(md.contains("new"));
    }

    #[test]
    fn empty_history_says_no_history() {
        let cur = snap("2026-06-07", 40.0);
        let dd = diff::diff(None, &cur);
        let md = render_markdown(&cur, &dd, &[]);
        assert!(md.contains("No history yet"));
    }
}
