// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! Rendering a [`Snapshot`] as Prometheus text-exposition metrics.
//!
//! We render the format by hand rather than pulling in the `prometheus` crate:
//! the output is a flat set of gauges and hand-rendering keeps the crate
//! dependency-light and the output fully unit-testable.

use std::fmt::Write as _;

use crate::snapshot::Snapshot;

/// Escape a Prometheus label value (`\`, `"`, newline).
fn esc(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Render `snap` into the Prometheus text exposition format.
#[must_use]
#[allow(clippy::too_many_lines)] // one flat block of gauge definitions
pub fn render_prometheus(snap: &Snapshot) -> String {
    let mut out = String::new();
    let project = esc(&snap.project);

    let gauge = |out: &mut String, name: &str, help: &str| {
        let _ = writeln!(out, "# HELP {name} {help}");
        let _ = writeln!(out, "# TYPE {name} gauge");
    };

    macro_rules! per_sub {
        ($name:literal, $help:literal, $field:expr) => {{
            gauge(&mut out, $name, $help);
            for m in &snap.subsystems {
                let _ = writeln!(
                    out,
                    "{}{{project=\"{}\",subsystem=\"{}\",group=\"{}\"}} {}",
                    $name,
                    project,
                    esc(&m.name),
                    esc(&m.group),
                    $field(m),
                );
            }
        }};
    }

    per_sub!(
        "cave_home_tracker_upstream_loc",
        "Upstream source LOC being ported",
        |m: &crate::snapshot::SubsystemMetric| m.upstream_loc
    );
    per_sub!(
        "cave_home_tracker_port_loc",
        "cave-home port source LOC",
        |m: &crate::snapshot::SubsystemMetric| m.port_loc
    );
    per_sub!(
        "cave_home_tracker_port_ratio",
        "port_loc / upstream_loc (capped at 1.0)",
        |m: &crate::snapshot::SubsystemMetric| m.ratio
    );
    per_sub!(
        "cave_home_tracker_tests_passed",
        "Passing port tests",
        |m: &crate::snapshot::SubsystemMetric| m.tests_passed
    );
    per_sub!(
        "cave_home_tracker_tests_failed",
        "Failing port tests",
        |m: &crate::snapshot::SubsystemMetric| m.tests_failed
    );
    per_sub!(
        "cave_home_tracker_tests_ignored",
        "Ignored port tests",
        |m: &crate::snapshot::SubsystemMetric| m.tests_ignored
    );
    per_sub!(
        "cave_home_tracker_test_pass_rate",
        "Fraction of run tests passing",
        |m: &crate::snapshot::SubsystemMetric| m.test_pass_rate
    );
    per_sub!(
        "cave_home_tracker_stub_count",
        "todo!/unimplemented!/panic! markers in port",
        |m: &crate::snapshot::SubsystemMetric| m.stubs.total()
    );
    per_sub!(
        "cave_home_tracker_real_pct",
        "Honest completion percent [0,100]",
        |m: &crate::snapshot::SubsystemMetric| m.real_pct
    );

    gauge(
        &mut out,
        "cave_home_tracker_group_real_pct",
        "Weighted honest completion per group",
    );
    for group in snap.groups() {
        let _ = writeln!(
            out,
            "cave_home_tracker_group_real_pct{{project=\"{}\",group=\"{}\"}} {}",
            project,
            esc(&group),
            snap.group_real_pct(&group),
        );
    }

    gauge(
        &mut out,
        "cave_home_tracker_overall_real_pct",
        "Weighted honest completion across all subsystems",
    );
    let _ = writeln!(
        out,
        "cave_home_tracker_overall_real_pct{{project=\"{}\"}} {}",
        project,
        snap.overall_real_pct(),
    );

    gauge(
        &mut out,
        "cave_home_tracker_subsystems",
        "Number of tracked subsystems",
    );
    let _ = writeln!(
        out,
        "cave_home_tracker_subsystems{{project=\"{}\"}} {}",
        project,
        snap.subsystems.len(),
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SubsystemMetric;
    use crate::stubs::StubCount;

    fn snap() -> Snapshot {
        Snapshot {
            project: "cave-home".into(),
            date: "2026-06-07".into(),
            generated_at: "2026-06-07T06:00:00Z".into(),
            subsystems: vec![
                SubsystemMetric::derive(
                    "kine",
                    "k3s",
                    1000,
                    500,
                    true,
                    8,
                    2,
                    1,
                    StubCount {
                        todo: 1,
                        ..StubCount::default()
                    },
                ),
                SubsystemMetric::derive(
                    "hue",
                    "smart-home",
                    0,
                    100,
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
    fn renders_help_type_and_values() {
        let text = render_prometheus(&snap());
        assert!(text.contains("# HELP cave_home_tracker_real_pct"));
        assert!(text.contains("# TYPE cave_home_tracker_real_pct gauge"));
        assert!(text.contains(
            "cave_home_tracker_port_loc{project=\"cave-home\",subsystem=\"kine\",group=\"k3s\"} 500"
        ));
        assert!(text.contains(
            "cave_home_tracker_stub_count{project=\"cave-home\",subsystem=\"kine\",group=\"k3s\"} 1"
        ));
    }

    #[test]
    fn renders_group_and_overall_rollups() {
        let text = render_prometheus(&snap());
        assert!(
            text.contains("cave_home_tracker_group_real_pct{project=\"cave-home\",group=\"k3s\"}")
        );
        assert!(text.contains(
            "cave_home_tracker_group_real_pct{project=\"cave-home\",group=\"smart-home\"}"
        ));
        assert!(text.contains("cave_home_tracker_overall_real_pct{project=\"cave-home\"}"));
        assert!(text.contains("cave_home_tracker_subsystems{project=\"cave-home\"} 2"));
    }

    #[test]
    fn label_values_are_escaped() {
        let mut s = snap();
        s.subsystems[0].name = "we\"ird".into();
        let text = render_prometheus(&s);
        assert!(text.contains("subsystem=\"we\\\"ird\""));
    }
}
