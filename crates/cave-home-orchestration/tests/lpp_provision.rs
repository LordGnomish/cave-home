// SPDX-License-Identifier: Apache-2.0
//! RED — failing tests for the local-path-provisioner **Provision decision**
//! (port of rancher/local-path-provisioner v0.0.36 `provisionFor`): PVC
//! validation (selector / access mode / node), volume-type resolution, node
//! affinity, and the resulting `PersistentVolume` spec. References API not yet
//! implemented.

use cave_home_orchestration::local_path_provisioner::config::{ConfigError, StorageClassConfig};
use cave_home_orchestration::local_path_provisioner::provision::{
    AccessMode, ProvisionError, ProvisionRequest, ProvisioningState, ReclaimPolicy, VolumeMode,
    VolumeSource, decide_provision,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn local_cfg() -> Result<StorageClassConfig, ConfigError> {
    StorageClassConfig::canonicalize(&[("node1", &["/opt/lpp"][..])], "")
}

#[test]
fn happy_path_hostpath_pv_spec() -> TestResult {
    let req = ProvisionRequest::new("pvc-123", "default", "data", "node1")
        .with_capacity_bytes(1024)
        .with_reclaim_policy(ReclaimPolicy::Delete);
    let out = decide_provision(&req, &local_cfg()?, 0)?;

    assert_eq!(out.state, ProvisioningState::Finished);
    // path = filepath.Join(basePath, "<pv>_<ns>_<pvc>")
    assert_eq!(out.volume_path, "/opt/lpp/pvc-123_default_data");

    let pv = &out.pv;
    assert_eq!(pv.name, "pvc-123");
    assert_eq!(pv.selected_node_annotation, "node1");
    assert_eq!(pv.reclaim_policy, ReclaimPolicy::Delete);
    assert_eq!(pv.access_modes, vec![AccessMode::ReadWriteOnce]);
    assert_eq!(pv.volume_mode, VolumeMode::Filesystem); // PV is always Filesystem
    assert_eq!(pv.capacity_bytes, 1024);
    assert_eq!(
        pv.source,
        VolumeSource::HostPath {
            path: "/opt/lpp/pvc-123_default_data".to_string()
        }
    );
    // default node affinity: kubernetes.io/hostname In [node1]
    let aff = pv
        .node_affinity
        .as_ref()
        .ok_or("local FS pins node affinity")?;
    assert_eq!(aff.key, "kubernetes.io/hostname");
    assert_eq!(aff.values, vec!["node1".to_string()]);
    Ok(())
}

#[test]
fn selector_is_rejected() -> TestResult {
    let req = ProvisionRequest::new("pvc-1", "default", "data", "node1").with_selector(true);
    let err = decide_provision(&req, &local_cfg()?, 0)
        .err()
        .ok_or("expected an error")?;
    assert!(
        matches!(err, ProvisionError::SelectorNotSupported),
        "got {err:?}"
    );
    Ok(())
}

#[test]
fn access_modes_rwo_and_rwop_ok_others_rejected() -> TestResult {
    // RWOP (1.22+) is accepted.
    let rwop = ProvisionRequest::new("pvc-1", "default", "data", "node1")
        .with_access_modes(vec![AccessMode::ReadWriteOncePod]);
    assert!(decide_provision(&rwop, &local_cfg()?, 0).is_ok());

    // RWX is rejected.
    let rwx = ProvisionRequest::new("pvc-1", "default", "data", "node1")
        .with_access_modes(vec![AccessMode::ReadWriteMany]);
    let err = decide_provision(&rwx, &local_cfg()?, 0)
        .err()
        .ok_or("expected an error")?;
    assert!(
        matches!(err, ProvisionError::UnsupportedAccessMode { .. }),
        "got {err:?}"
    );
    Ok(())
}

#[test]
fn local_fs_requires_a_node() -> TestResult {
    let req = ProvisionRequest::new("pvc-1", "default", "data", ""); // empty node
    let err = decide_provision(&req, &local_cfg()?, 0)
        .err()
        .ok_or("expected an error")?;
    assert!(
        matches!(err, ProvisionError::NoNodeSpecified),
        "got {err:?}"
    );
    Ok(())
}

#[test]
fn shared_fs_has_no_node_affinity_and_no_node_requirement() -> TestResult {
    let shared = StorageClassConfig::canonicalize(&[], "/mnt/shared")?;
    // node empty is fine on shared FS; selector/access-mode checks are skipped.
    let req = ProvisionRequest::new("pvc-9", "team", "vol", "").with_selector(true);
    let out = decide_provision(&req, &shared, 0)?;
    assert_eq!(out.volume_path, "/mnt/shared/pvc-9_team_vol");
    assert!(
        out.pv.node_affinity.is_none(),
        "shared FS has no node affinity"
    );
    assert_eq!(
        out.pv.source,
        VolumeSource::HostPath {
            path: "/mnt/shared/pvc-9_team_vol".to_string()
        }
    );
    Ok(())
}

#[test]
fn volume_type_resolution_annotations_then_default() -> TestResult {
    // PVC annotation volumeType=local wins → Local source.
    let pvc_local = ProvisionRequest::new("pvc-1", "default", "data", "node1")
        .with_pvc_volume_type(Some("local".to_string()));
    let out = decide_provision(&pvc_local, &local_cfg()?, 0)?;
    assert!(
        matches!(out.pv.source, VolumeSource::Local { .. }),
        "got {:?}",
        out.pv.source
    );

    // SC defaultVolumeType=local annotation, no PVC override → Local.
    let sc_local = ProvisionRequest::new("pvc-1", "default", "data", "node1")
        .with_default_volume_type(Some("local".to_string()));
    assert!(matches!(
        decide_provision(&sc_local, &local_cfg()?, 0)?.pv.source,
        VolumeSource::Local { .. }
    ));

    // PVC override beats SC default.
    let both = ProvisionRequest::new("pvc-1", "default", "data", "node1")
        .with_default_volume_type(Some("local".to_string()))
        .with_pvc_volume_type(Some("hostPath".to_string()));
    assert!(matches!(
        decide_provision(&both, &local_cfg()?, 0)?.pv.source,
        VolumeSource::HostPath { .. }
    ));

    // unrecognized type → error.
    let bad = ProvisionRequest::new("pvc-1", "default", "data", "node1")
        .with_pvc_volume_type(Some("nfs".to_string()));
    let err = decide_provision(&bad, &local_cfg()?, 0)
        .err()
        .ok_or("expected an error")?;
    assert!(
        matches!(err, ProvisionError::UnrecognizedVolumeType { .. }),
        "got {err:?}"
    );
    Ok(())
}

#[test]
fn node_affinity_value_prefers_label_and_key_param_overrides() -> TestResult {
    // Affinity value comes from the node's label under the (overridable) key.
    let req = ProvisionRequest::new("pvc-1", "default", "data", "node1")
        .with_affinity_key_param(Some("topology.kubernetes.io/zone".to_string()))
        .with_node_label("topology.kubernetes.io/zone", "zone-a");
    let out = decide_provision(&req, &local_cfg()?, 0)?;
    let aff = out
        .pv
        .node_affinity
        .as_ref()
        .ok_or("expected Some, got None")?;
    assert_eq!(aff.key, "topology.kubernetes.io/zone");
    assert_eq!(aff.values, vec!["zone-a".to_string()]);

    // When the label is absent, the node name is used as the value.
    let req2 = ProvisionRequest::new("pvc-2", "default", "data", "node1")
        .with_affinity_key_param(Some("topology.kubernetes.io/zone".to_string()));
    let aff2_out = decide_provision(&req2, &local_cfg()?, 0)?;
    let aff2 = aff2_out
        .pv
        .node_affinity
        .as_ref()
        .ok_or("expected Some, got None")?;
    assert_eq!(aff2.values, vec!["node1".to_string()]);
    Ok(())
}
