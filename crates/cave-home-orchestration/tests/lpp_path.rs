// SPDX-License-Identifier: Apache-2.0
//! RED — failing tests for the local-path-provisioner **path selection** decision
//! (port of rancher/local-path-provisioner v0.0.36 `getPathOnNode`, the
//! `pvName_namespace_pvcName` folder name, `pathFromPattern`'s safe-prefix /
//! `filepath.IsLocal` check, and the `filepath.Join` volume path). References API
//! not yet implemented.

use cave_home_orchestration::local_path_provisioner::config::StorageClassConfig;
use cave_home_orchestration::local_path_provisioner::path::{
    base_path_on_node, folder_name, validate_pattern_path, volume_path, PathError,
};

#[test]
fn folder_name_joins_pv_namespace_pvc_with_underscore() {
    // Upstream: strings.Join([]string{pvName, namespace, pvcName}, "_").
    assert_eq!(folder_name("pvc-abc", "default", "data"), "pvc-abc_default_data");
}

#[test]
fn base_path_shared_fs_ignores_node() {
    let shared = StorageClassConfig::canonicalize(&[], "/mnt/shared").unwrap();
    // node is irrelevant; selector irrelevant; returns the shared path.
    assert_eq!(base_path_on_node(&shared, "anyNode", "", 0).unwrap(), "/mnt/shared");
}

#[test]
fn base_path_local_single_candidate() {
    let cfg = StorageClassConfig::canonicalize(&[("node1", &["/opt/lpp"][..])], "").unwrap();
    assert_eq!(base_path_on_node(&cfg, "node1", "", 0).unwrap(), "/opt/lpp");
}

#[test]
fn base_path_falls_back_to_default_non_listed_node() {
    let cfg = StorageClassConfig::canonicalize(
        &[("DEFAULT_PATH_FOR_NON_LISTED_NODES", &["/opt/lpp"][..])],
        "",
    )
    .unwrap();
    // node "worker7" is not listed → falls back to the DEFAULT entry.
    assert_eq!(base_path_on_node(&cfg, "worker7", "", 0).unwrap(), "/opt/lpp");
}

#[test]
fn base_path_unlisted_node_without_default_errors() {
    let cfg = StorageClassConfig::canonicalize(&[("node1", &["/opt/lpp"][..])], "").unwrap();
    let err = base_path_on_node(&cfg, "worker7", "", 0).unwrap_err();
    assert!(matches!(err, PathError::NoNodeConfigured { .. }), "got {err:?}");
}

#[test]
fn base_path_node_with_empty_paths_errors() {
    let cfg = StorageClassConfig::canonicalize(&[("node1", &[][..])], "").unwrap();
    let err = base_path_on_node(&cfg, "node1", "", 0).unwrap_err();
    assert!(matches!(err, PathError::NoLocalPath { .. }), "got {err:?}");
}

#[test]
fn base_path_requested_path_must_be_configured() {
    let cfg =
        StorageClassConfig::canonicalize(&[("node1", &["/data1", "/data2"][..])], "").unwrap();
    // requested path present in the set → returned (cleaned).
    assert_eq!(base_path_on_node(&cfg, "node1", "/data2", 0).unwrap(), "/data2");
    assert_eq!(base_path_on_node(&cfg, "node1", "/data2/", 0).unwrap(), "/data2");
    // requested path absent → error.
    let err = base_path_on_node(&cfg, "node1", "/nope", 0).unwrap_err();
    assert!(matches!(err, PathError::RequestedPathNotConfigured { .. }), "got {err:?}");
}

#[test]
fn base_path_selector_picks_deterministically_among_candidates() {
    let cfg =
        StorageClassConfig::canonicalize(&[("node1", &["/data1", "/data2", "/data3"][..])], "")
            .unwrap();
    // Candidates are sorted: ["/data1","/data2","/data3"]. Selector indexes into
    // them and wraps (upstream uses rand.IntN; cave-home externalizes the choice).
    assert_eq!(base_path_on_node(&cfg, "node1", "", 0).unwrap(), "/data1");
    assert_eq!(base_path_on_node(&cfg, "node1", "", 1).unwrap(), "/data2");
    assert_eq!(base_path_on_node(&cfg, "node1", "", 2).unwrap(), "/data3");
    assert_eq!(base_path_on_node(&cfg, "node1", "", 3).unwrap(), "/data1"); // wraps
}

#[test]
fn volume_path_joins_and_cleans() {
    // Upstream: filepath.Join(basePath, folderName).
    assert_eq!(volume_path("/opt/lpp", "pvc-abc_default_data"), "/opt/lpp/pvc-abc_default_data");
    assert_eq!(volume_path("/opt/lpp/", "sub/"), "/opt/lpp/sub");
}

#[test]
fn validate_pattern_path_enforces_safe_prefix_and_locality() {
    // Upstream pathFromPattern (!allowUnsafe): rendered path must start with
    // "<namespace>/<pvcName>/" and must be filepath.IsLocal.
    assert!(validate_pattern_path("default/data/sub", "default", "data", false).is_ok());

    // missing the required namespace/pvc prefix → rejected.
    let bad_prefix = validate_pattern_path("other/x/sub", "default", "data", false).unwrap_err();
    assert!(matches!(bad_prefix, PathError::UnsafePathPattern { .. }), "got {bad_prefix:?}");

    // A `..` count that nets out within the subtree (clean -> "etc") is LOCAL and
    // is accepted, matching filepath.IsLocal (which cleans before judging).
    assert!(validate_pattern_path("default/data/../../etc", "default", "data", false).is_ok());

    // A path that escapes the subtree (one `..` past its depth) is non-local and
    // is rejected even though it textually starts with the namespace/pvc prefix.
    let traversal =
        validate_pattern_path("default/data/../../../etc", "default", "data", false).unwrap_err();
    assert!(matches!(traversal, PathError::UnsafePathPattern { .. }), "got {traversal:?}");

    // allowUnsafe bypasses both checks.
    assert!(validate_pattern_path("/anywhere", "default", "data", true).is_ok());
}
