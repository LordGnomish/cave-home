// SPDX-License-Identifier: Apache-2.0
//! RED — failing tests for the local-path-provisioner **Delete / reclaim**
//! decision (port of rancher/local-path-provisioner v0.0.36 `deleteFor` +
//! `getPathAndNodeForPV`). References API not yet implemented.

use cave_home_orchestration::local_path_provisioner::provision::{
    AccessMode, NodeAffinityTerm, PvSpec, ReclaimPolicy, VolumeMode, VolumeSource,
};
use cave_home_orchestration::local_path_provisioner::reclaim::{
    DeleteDecision, ReclaimError, delete_decision, path_and_node_for_pv,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn pv(reclaim: ReclaimPolicy, affinity: Option<NodeAffinityTerm>) -> PvSpec {
    PvSpec {
        name: "pvc-123".to_string(),
        selected_node_annotation: "node1".to_string(),
        reclaim_policy: reclaim,
        access_modes: vec![AccessMode::ReadWriteOnce],
        volume_mode: VolumeMode::Filesystem,
        capacity_bytes: 1024,
        source: VolumeSource::HostPath {
            path: "/opt/lpp/pvc-123_default_data".to_string(),
        },
        node_affinity: affinity,
    }
}

fn affinity_node(node: &str) -> NodeAffinityTerm {
    NodeAffinityTerm {
        key: "kubernetes.io/hostname".to_string(),
        values: vec![node.to_string()],
    }
}

#[test]
fn retain_policy_keeps_the_volume() -> TestResult {
    let pv = pv(ReclaimPolicy::Retain, Some(affinity_node("node1")));
    assert_eq!(delete_decision(&pv, false)?, DeleteDecision::Retain);
    Ok(())
}

#[test]
fn delete_policy_local_fs_tears_down_with_path_and_node() -> TestResult {
    let pv = pv(ReclaimPolicy::Delete, Some(affinity_node("node1")));
    assert_eq!(
        delete_decision(&pv, false)?,
        DeleteDecision::Teardown {
            path: "/opt/lpp/pvc-123_default_data".to_string(),
            node: "node1".to_string(),
        }
    );
    Ok(())
}

#[test]
fn recycle_policy_also_tears_down_since_not_retain() -> TestResult {
    let pv = pv(ReclaimPolicy::Recycle, Some(affinity_node("node1")));
    assert!(matches!(
        delete_decision(&pv, false)?,
        DeleteDecision::Teardown { .. }
    ));
    Ok(())
}

#[test]
fn delete_policy_shared_fs_tears_down_without_a_node() -> TestResult {
    // Shared FS PVs carry no node affinity; node comes back empty.
    let pv = pv(ReclaimPolicy::Delete, None);
    assert_eq!(
        delete_decision(&pv, true)?,
        DeleteDecision::Teardown {
            path: "/opt/lpp/pvc-123_default_data".to_string(),
            node: String::new(),
        }
    );
    Ok(())
}

#[test]
fn path_and_node_extracts_from_local_source() -> TestResult {
    let mut pv = pv(ReclaimPolicy::Delete, Some(affinity_node("node2")));
    pv.source = VolumeSource::Local {
        path: "/data/vol".to_string(),
    };
    assert_eq!(
        path_and_node_for_pv(&pv, false)?,
        ("/data/vol".to_string(), "node2".to_string())
    );
    Ok(())
}

#[test]
fn missing_node_affinity_on_local_fs_errors() -> TestResult {
    let pv = pv(ReclaimPolicy::Delete, None);
    let err = path_and_node_for_pv(&pv, false)
        .err()
        .ok_or("expected an error")?;
    assert!(matches!(err, ReclaimError::NoNodeAffinity), "got {err:?}");
    Ok(())
}

#[test]
fn multiple_affinity_values_error() -> TestResult {
    let multi = NodeAffinityTerm {
        key: "kubernetes.io/hostname".to_string(),
        values: vec!["node1".to_string(), "node2".to_string()],
    };
    let pv = pv(ReclaimPolicy::Delete, Some(multi));
    let err = path_and_node_for_pv(&pv, false)
        .err()
        .ok_or("expected an error")?;
    assert!(
        matches!(err, ReclaimError::MultipleAffinityValues),
        "got {err:?}"
    );
    Ok(())
}

#[test]
fn empty_affinity_value_cannot_find_node() -> TestResult {
    let pv = pv(ReclaimPolicy::Delete, Some(affinity_node("")));
    let err = path_and_node_for_pv(&pv, false)
        .err()
        .ok_or("expected an error")?;
    assert!(
        matches!(err, ReclaimError::CannotFindAffinitedNode),
        "got {err:?}"
    );
    Ok(())
}
