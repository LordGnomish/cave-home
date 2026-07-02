// SPDX-License-Identifier: Apache-2.0
//! RED — failing tests for the local-path-provisioner **observability metrics**
//! (Track 4): PV count by status, provisioning latency, and the reconcile-error
//! counter, rendered as Prometheus text exposition. References API not yet
//! implemented.

use cave_home_orchestration::local_path_provisioner::metrics::{LocalPathMetrics, PvPhase};

#[test]
fn pv_phase_ids_match_kubernetes() {
    assert_eq!(PvPhase::Pending.as_str(), "Pending");
    assert_eq!(PvPhase::Available.as_str(), "Available");
    assert_eq!(PvPhase::Bound.as_str(), "Bound");
    assert_eq!(PvPhase::Released.as_str(), "Released");
    assert_eq!(PvPhase::Failed.as_str(), "Failed");
}

#[test]
fn empty_metrics_are_all_zero() {
    let m = LocalPathMetrics::observe(&[]);
    assert_eq!(m, LocalPathMetrics::default());
    assert_eq!(m.pvs_total, 0);
    assert_eq!(m.pvs_by_phase(PvPhase::Bound), 0);
}

#[test]
fn counts_pvs_by_phase() {
    let m = LocalPathMetrics::observe(&[
        PvPhase::Bound,
        PvPhase::Bound,
        PvPhase::Released,
        PvPhase::Failed,
        PvPhase::Available,
    ]);
    assert_eq!(m.pvs_total, 5);
    assert_eq!(m.pvs_by_phase(PvPhase::Bound), 2);
    assert_eq!(m.pvs_by_phase(PvPhase::Released), 1);
    assert_eq!(m.pvs_by_phase(PvPhase::Failed), 1);
    assert_eq!(m.pvs_by_phase(PvPhase::Available), 1);
    assert_eq!(m.pvs_by_phase(PvPhase::Pending), 0);
}

#[test]
fn records_provision_deletion_and_reconcile_error_counters() {
    let m = LocalPathMetrics::observe(&[PvPhase::Bound])
        .with_provisions(10, 2)
        .with_deletions(3)
        .with_reconcile_errors(1);
    assert_eq!(m.provisions_total, 10);
    assert_eq!(m.provision_failures_total, 2);
    assert_eq!(m.deletions_total, 3);
    assert_eq!(m.reconcile_errors_total, 1);
}

#[test]
fn records_provisioning_latency_summary() {
    let m = LocalPathMetrics::observe(&[])
        .record_latency_seconds(0.5)
        .record_latency_seconds(1.5);
    assert_eq!(m.provision_latency_seconds_count, 2);
    assert!((m.provision_latency_seconds_sum - 2.0).abs() < 1e-9);
}

#[test]
fn prometheus_exposition_lists_phase_counters_latency_and_errors() {
    let text = LocalPathMetrics::observe(&[PvPhase::Bound, PvPhase::Bound, PvPhase::Released])
        .with_provisions(7, 1)
        .with_deletions(2)
        .with_reconcile_errors(3)
        .record_latency_seconds(0.25)
        .to_prometheus();

    // PV count by status (labeled gauge).
    assert!(text.contains("# TYPE localpath_pvs gauge"), "{text}");
    assert!(text.contains("localpath_pvs{phase=\"Bound\"} 2"), "{text}");
    assert!(text.contains("localpath_pvs{phase=\"Released\"} 1"), "{text}");
    assert!(text.contains("localpath_pvs{phase=\"Pending\"} 0"), "{text}");
    assert!(text.contains("localpath_pvs_total 3"), "{text}");

    // counters.
    assert!(text.contains("# TYPE localpath_provisions_total counter"), "{text}");
    assert!(text.contains("localpath_provisions_total 7"), "{text}");
    assert!(text.contains("localpath_provision_failures_total 1"), "{text}");
    assert!(text.contains("localpath_deletions_total 2"), "{text}");
    assert!(text.contains("localpath_reconcile_errors_total 3"), "{text}");

    // provisioning latency summary (sum + count).
    assert!(text.contains("# TYPE localpath_provision_latency_seconds summary"), "{text}");
    assert!(text.contains("localpath_provision_latency_seconds_count 1"), "{text}");
    assert!(text.contains("localpath_provision_latency_seconds_sum"), "{text}");

    // Every non-comment line is "metric value" (value parses as a number).
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let parts: Vec<&str> = line.rsplitn(2, ' ').collect();
        assert_eq!(parts.len(), 2, "bad exposition line: {line:?}");
        assert!(parts[0].parse::<f64>().is_ok(), "non-numeric value: {line:?}");
    }
}
