// SPDX-License-Identifier: Apache-2.0
//! RED — failing tests for the local-path-provisioner **report view-model**: the
//! PV/PVC rows that `cavectl orchestration storage list-pvs|describe` and the
//! Portal Storage page render. References API not yet implemented.

use cave_home_orchestration::local_path_provisioner::metrics::PvPhase;
use cave_home_orchestration::local_path_provisioner::provision::{
    AccessMode, NodeAffinityTerm, PvSpec, ReclaimPolicy, VolumeMode, VolumeSource,
};
use cave_home_orchestration::local_path_provisioner::report::{PvRow, StorageReport};

fn pv_spec(name: &str, source: VolumeSource, affinity: Option<NodeAffinityTerm>) -> PvSpec {
    PvSpec {
        name: name.to_string(),
        selected_node_annotation: "node1".to_string(),
        reclaim_policy: ReclaimPolicy::Delete,
        access_modes: vec![AccessMode::ReadWriteOnce],
        volume_mode: VolumeMode::Filesystem,
        capacity_bytes: 2048,
        source,
        node_affinity: affinity,
    }
}

#[test]
fn row_derives_host_path_and_node_from_local_fs_pv() {
    let pv = pv_spec(
        "pvc-1",
        VolumeSource::HostPath { path: "/opt/lpp/pvc-1_default_data".to_string() },
        Some(NodeAffinityTerm {
            key: "kubernetes.io/hostname".to_string(),
            values: vec!["node1".to_string()],
        }),
    );
    let row = PvRow::from_pv_spec(&pv, "default", "data", PvPhase::Bound);
    assert_eq!(row.name, "pvc-1");
    assert_eq!(row.pvc_namespace, "default");
    assert_eq!(row.pvc_name, "data");
    assert_eq!(row.host_path, "/opt/lpp/pvc-1_default_data");
    assert_eq!(row.node, "node1");
    assert_eq!(row.phase, PvPhase::Bound);
    assert_eq!(row.capacity_bytes, 2048);
    assert_eq!(row.reclaim_policy, ReclaimPolicy::Delete);
}

#[test]
fn row_handles_shared_fs_pv_with_no_affinity() {
    let pv = pv_spec(
        "pvc-2",
        VolumeSource::Local { path: "/mnt/shared/pvc-2_team_vol".to_string() },
        None,
    );
    let row = PvRow::from_pv_spec(&pv, "team", "vol", PvPhase::Available);
    assert_eq!(row.host_path, "/mnt/shared/pvc-2_team_vol");
    assert_eq!(row.node, ""); // shared FS: no node pin
}

#[test]
fn report_lists_rows_and_describes_by_pvc() {
    let rows = vec![
        PvRow::from_pv_spec(
            &pv_spec("pv-a", VolumeSource::HostPath { path: "/opt/a".to_string() }, None),
            "default",
            "claim-a",
            PvPhase::Bound,
        ),
        PvRow::from_pv_spec(
            &pv_spec("pv-b", VolumeSource::HostPath { path: "/opt/b".to_string() }, None),
            "team",
            "claim-b",
            PvPhase::Released,
        ),
    ];
    let report = StorageReport::new(rows);
    assert_eq!(report.len(), 2);
    assert!(!report.is_empty());

    // describe <pvc> — namespaced lookup.
    let found = report.find_by_pvc("team", "claim-b").expect("present");
    assert_eq!(found.name, "pv-b");
    assert_eq!(found.host_path, "/opt/b");
    assert!(report.find_by_pvc("team", "nope").is_none());
    assert!(report.find_by_pvc("default", "claim-b").is_none()); // namespace matters
}

#[test]
fn empty_report_is_empty() {
    let report = StorageReport::new(vec![]);
    assert!(report.is_empty());
    assert_eq!(report.len(), 0);
    assert!(report.find_by_pvc("default", "x").is_none());
}
