// SPDX-License-Identifier: Apache-2.0
//! RED — failing tests for the local-path-provisioner **Delete / reclaim**
//! decision (port of rancher/local-path-provisioner v0.0.36 `deleteFor` +
//! `getPathAndNodeForPV`). References API not yet implemented.

use cave_home_orchestration::local_path_provisioner::provision::{
    AccessMode, NodeAffinityTerm, PvSpec, ReclaimPolicy, VolumeMode, VolumeSource,
};
use cave_home_orchestration::local_path_provisioner::reclaim::{
    delete_decision, path_and_node_for_pv, DeleteDecision, ReclaimError,
};

fn pv(reclaim: ReclaimPolicy, affinity: Option<NodeAffinityTerm>) -> PvSpec {
    PvSpec {
        name: "pvc-123".to_string(),
        selected_node_annotation: "node1".to_string(),
        reclaim_policy: reclaim,
        access_modes: vec![AccessMode::ReadWriteOnce],
        volume_mode: VolumeMode::Filesystem,
        capacity_bytes: 1024,
        source: VolumeSource::HostPath { path: "/opt/lpp/pvc-123_default_data".to_string() },
        node_affinity: affinity,
    }
}

fn affinity_node(node: &str) -> NodeAffinityTerm {
    NodeAffinityTerm { key: "kubernetes.io/hostname".to_string(), values: vec![node.to_string()] }
}

#[test]
fn retain_policy_keeps_the_volume() {
    let pv = pv(ReclaimPolicy::Retain, Some(affinity_node("node1")));
    assert_eq!(delete_decision(&pv, false).unwrap(), DeleteDecision::Retain);
}

#[test]
fn delete_policy_local_fs_tears_down_with_path_and_node() {
    let pv = pv(ReclaimPolicy::Delete, Some(affinity_node("node1")));
    assert_eq!(
        delete_decision(&pv, false).unwrap(),
        DeleteDecision::Teardown {
            path: "/opt/lpp/pvc-123_default_data".to_string(),
            node: "node1".to_string(),
        }
    );
}

#[test]
fn recycle_policy_also_tears_down_since_not_retain() {
    let pv = pv(ReclaimPolicy::Recycle, Some(affinity_node("node1")));
    assert!(matches!(delete_decision(&pv, false).unwrap(), DeleteDecision::Teardown { .. }));
}

#[test]
fn delete_policy_shared_fs_tears_down_without_a_node() {
    // Shared FS PVs carry no node affinity; node comes back empty.
    let pv = pv(ReclaimPolicy::Delete, None);
    assert_eq!(
        delete_decision(&pv, true).unwrap(),
        DeleteDecision::Teardown {
            path: "/opt/lpp/pvc-123_default_data".to_string(),
            node: String::new(),
        }
    );
}

#[test]
fn path_and_node_extracts_from_local_source() {
    let mut pv = pv(ReclaimPolicy::Delete, Some(affinity_node("node2")));
    pv.source = VolumeSource::Local { path: "/data/vol".to_string() };
    assert_eq!(
        path_and_node_for_pv(&pv, false).unwrap(),
        ("/data/vol".to_string(), "node2".to_string())
    );
}

#[test]
fn missing_node_affinity_on_local_fs_errors() {
    let pv = pv(ReclaimPolicy::Delete, None);
    let err = path_and_node_for_pv(&pv, false).unwrap_err();
    assert!(matches!(err, ReclaimError::NoNodeAffinity), "got {err:?}");
}

#[test]
fn multiple_affinity_values_error() {
    let multi = NodeAffinityTerm {
        key: "kubernetes.io/hostname".to_string(),
        values: vec!["node1".to_string(), "node2".to_string()],
    };
    let pv = pv(ReclaimPolicy::Delete, Some(multi));
    let err = path_and_node_for_pv(&pv, false).unwrap_err();
    assert!(matches!(err, ReclaimError::MultipleAffinityValues), "got {err:?}");
}

#[test]
fn empty_affinity_value_cannot_find_node() {
    let pv = pv(ReclaimPolicy::Delete, Some(affinity_node("")));
    let err = path_and_node_for_pv(&pv, false).unwrap_err();
    assert!(matches!(err, ReclaimError::CannotFindAffinitedNode), "got {err:?}");
}
