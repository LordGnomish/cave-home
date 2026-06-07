// SPDX-License-Identifier: Apache-2.0
//! RED — failing tests for the local-path-provisioner **config canonicalization**
//! decision logic (port of rancher/local-path-provisioner v0.0.36
//! `canonicalizeStorageClassConfig` / `canonicalizeConfig` / `isSharedFilesystem`
//! / `pickConfig`). References API not yet implemented, so this file does not
//! compile until the GREEN commit lands `local_path_provisioner::config`.

use cave_home_orchestration::local_path_provisioner::config::{
    ActionType, ConfigError, NodePathMap, ProvisionerConfig, StorageClassConfig,
};
use cave_home_orchestration::local_path_provisioner::{
    DEFAULT_CMD_TIMEOUT_SECONDS, NODE_DEFAULT_NON_LISTED_NODES,
};

#[test]
fn action_type_ids_match_upstream() {
    // Upstream constants: ActionTypeCreate="create", ActionTypeDelete="delete".
    assert_eq!(ActionType::Create.as_str(), "create");
    assert_eq!(ActionType::Delete.as_str(), "delete");
}

#[test]
fn default_non_listed_node_key_matches_upstream() {
    assert_eq!(NODE_DEFAULT_NON_LISTED_NODES, "DEFAULT_PATH_FOR_NON_LISTED_NODES");
}

#[test]
fn node_path_map_accepts_absolute_paths_and_dedups_lexically() {
    // "/opt/x" and "/opt/x/" canonicalize to the same path → duplicate.
    let err = NodePathMap::canonicalize("nodeA", &["/opt/x", "/opt/x/"]).unwrap_err();
    assert!(matches!(err, ConfigError::DuplicatePath { .. }), "got {err:?}");

    // Two genuinely distinct paths are kept, sorted for deterministic selection.
    let npm = NodePathMap::canonicalize("nodeA", &["/data2", "/data1"]).unwrap();
    assert_eq!(npm.paths(), &["/data1".to_string(), "/data2".to_string()]);
    assert!(npm.contains("/data1"));
    assert!(!npm.contains("/nope"));
}

#[test]
fn node_path_map_rejects_relative_and_root() {
    // Upstream: `if p[0] != '/'` → "path must start with /".
    let rel = NodePathMap::canonicalize("nodeA", &["opt/x"]).unwrap_err();
    assert!(matches!(rel, ConfigError::PathNotAbsolute { .. }), "got {rel:?}");

    // Upstream: `if path == "/"` → "cannot use root ('/') as path".
    let root = NodePathMap::canonicalize("nodeA", &["/"]).unwrap_err();
    assert!(matches!(root, ConfigError::RootPath { .. }), "got {root:?}");

    // `/opt/..` cleans to `/` and is likewise rejected.
    let cleans_to_root = NodePathMap::canonicalize("nodeA", &["/opt/.."]).unwrap_err();
    assert!(matches!(cleans_to_root, ConfigError::RootPath { .. }), "got {cleans_to_root:?}");
}

#[test]
fn node_path_map_empty_paths_is_a_valid_no_provision_node() {
    // Upstream allows a node with an empty path list (provisioning is refused
    // there later, in getPathOnNode); canonicalization itself must succeed.
    let npm = NodePathMap::canonicalize("nodeNoStorage", &[]).unwrap();
    assert!(npm.is_empty());
}

#[test]
fn storage_class_config_rejects_duplicate_nodes() {
    let err = StorageClassConfig::canonicalize(
        &[("nodeA", &["/data1"][..]), ("nodeA", &["/data2"][..])],
        "",
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::DuplicateNode { .. }), "got {err:?}");
}

#[test]
fn is_shared_filesystem_decision_table() {
    // nodePathMap set, shared empty → local FS (false).
    let local = StorageClassConfig::canonicalize(&[("nodeA", &["/data1"][..])], "").unwrap();
    assert_eq!(local.is_shared_filesystem(), Ok(false));

    // shared set, no nodePathMap → shared (true).
    let shared = StorageClassConfig::canonicalize(&[], "/mnt/shared").unwrap();
    assert_eq!(shared.is_shared_filesystem(), Ok(true));

    // both set → error.
    let both = StorageClassConfig::canonicalize(&[("nodeA", &["/data1"][..])], "/mnt/shared")
        .unwrap_err();
    assert!(matches!(both, ConfigError::BothNodeMapAndSharedFs), "got {both:?}");

    // neither set → error.
    let neither = StorageClassConfig::canonicalize(&[], "").unwrap();
    assert_eq!(neither.is_shared_filesystem(), Err(ConfigError::NeitherNodeMapNorSharedFs));
}

#[test]
fn provisioner_config_pick_default_when_no_named_classes() {
    // pickConfig: no StorageClassConfigs → always the default class config.
    let default_class = StorageClassConfig::canonicalize(&[("nodeA", &["/data1"][..])], "").unwrap();
    let cfg = ProvisionerConfig::new(default_class.clone(), 0);
    // any storage class name resolves to the single default config.
    assert_eq!(cfg.pick("anything").unwrap(), &default_class);
    // cmdTimeoutSeconds 0 is canonicalized to the upstream default (120).
    assert_eq!(cfg.cmd_timeout_seconds(), DEFAULT_CMD_TIMEOUT_SECONDS);
    assert_eq!(DEFAULT_CMD_TIMEOUT_SECONDS, 120);
}

#[test]
fn provisioner_config_pick_named_class_or_error() {
    let class_a = StorageClassConfig::canonicalize(&[("nodeA", &["/a"][..])], "").unwrap();
    let class_b = StorageClassConfig::canonicalize(&[("nodeB", &["/b"][..])], "").unwrap();
    let cfg = ProvisionerConfig::with_classes(
        [("fast".to_string(), class_a.clone()), ("slow".to_string(), class_b.clone())],
        90,
    );
    assert_eq!(cfg.pick("fast").unwrap(), &class_a);
    assert_eq!(cfg.pick("slow").unwrap(), &class_b);
    // explicit non-zero timeout is preserved.
    assert_eq!(cfg.cmd_timeout_seconds(), 90);

    let err = cfg.pick("missing").unwrap_err();
    assert!(matches!(err, ConfigError::UnknownStorageClass { .. }), "got {err:?}");
}
