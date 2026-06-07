// SPDX-License-Identifier: Apache-2.0
//! RED — failing tests for the local-path-provisioner **helper-pod command**
//! decision (port of rancher/local-path-provisioner v0.0.36 `createHelperPod`'s
//! pure command/env/args/name construction). References API not yet implemented.

use cave_home_orchestration::local_path_provisioner::config::{ActionType, StorageClassConfig};
use cave_home_orchestration::local_path_provisioner::helper::{
    build_helper_command, HelperError, VolumeOptions, DEFAULT_SETUP_SCRIPT, DEFAULT_TEARDOWN_SCRIPT,
};
use cave_home_orchestration::local_path_provisioner::provision::VolumeMode;
use cave_home_orchestration::local_path_provisioner::ProvisionerConfig;

fn local() -> StorageClassConfig {
    StorageClassConfig::canonicalize(&[("node1", &["/opt/lpp"][..])], "").unwrap()
}

fn opts() -> VolumeOptions {
    VolumeOptions {
        name: "pvc-123".to_string(),
        path: "/opt/lpp/pvc-123_default_data".to_string(),
        mode: VolumeMode::Filesystem,
        size_bytes: 1024,
        node: "node1".to_string(),
    }
}

#[test]
fn create_uses_default_setup_command_when_unset() {
    let cfg = ProvisionerConfig::new(local(), 0);
    let cmd = build_helper_command(ActionType::Create, "helper-pod", &opts(), &cfg, false).unwrap();
    // Upstream: provisionCmd defaults to ["/bin/sh", "/script/setup"].
    assert_eq!(cmd.command, vec!["/bin/sh".to_string(), "/script/setup".to_string()]);
}

#[test]
fn delete_uses_default_teardown_command_when_unset() {
    let cfg = ProvisionerConfig::new(local(), 0);
    let cmd = build_helper_command(ActionType::Delete, "helper-pod", &opts(), &cfg, false).unwrap();
    assert_eq!(cmd.command, vec!["/bin/sh".to_string(), "/script/teardown".to_string()]);
}

#[test]
fn custom_setup_teardown_commands_are_used_verbatim() {
    let cfg = ProvisionerConfig::new(local(), 0).with_commands("/custom/setup.sh", "/custom/tear.sh");
    let c = build_helper_command(ActionType::Create, "helper-pod", &opts(), &cfg, false).unwrap();
    assert_eq!(c.command, vec!["/custom/setup.sh".to_string()]);
    let d = build_helper_command(ActionType::Delete, "helper-pod", &opts(), &cfg, false).unwrap();
    assert_eq!(d.command, vec!["/custom/tear.sh".to_string()]);
}

#[test]
fn env_triplet_matches_upstream() {
    let cfg = ProvisionerConfig::new(local(), 0);
    let cmd = build_helper_command(ActionType::Create, "helper-pod", &opts(), &cfg, false).unwrap();
    // VOL_DIR = join(parentDir, volumeDir) = the cleaned volume path.
    assert_eq!(cmd.env_var("VOL_DIR"), Some("/opt/lpp/pvc-123_default_data"));
    assert_eq!(cmd.env_var("VOL_MODE"), Some("Filesystem"));
    assert_eq!(cmd.env_var("VOL_SIZE_BYTES"), Some("1024"));
}

#[test]
fn args_match_upstream_flag_order() {
    let cfg = ProvisionerConfig::new(local(), 0);
    let cmd = build_helper_command(ActionType::Create, "helper-pod", &opts(), &cfg, false).unwrap();
    // Upstream args: -p <path> -s <size> -m <mode> -a <action>.
    assert_eq!(
        cmd.args,
        vec![
            "-p".to_string(),
            "/opt/lpp/pvc-123_default_data".to_string(),
            "-s".to_string(),
            "1024".to_string(),
            "-m".to_string(),
            "Filesystem".to_string(),
            "-a".to_string(),
            "create".to_string(),
        ]
    );
}

#[test]
fn pod_name_is_base_action_name_and_truncated_to_128() {
    let cfg = ProvisionerConfig::new(local(), 0);
    let cmd = build_helper_command(ActionType::Create, "helper-pod", &opts(), &cfg, false).unwrap();
    assert_eq!(cmd.pod_name, "helper-pod-create-pvc-123");

    // Long PV name → the assembled name is byte-truncated to 128.
    let mut long = opts();
    long.name = "x".repeat(200);
    let cmd2 = build_helper_command(ActionType::Create, "helper-pod", &long, &cfg, false).unwrap();
    assert_eq!(cmd2.pod_name.len(), 128);
    assert!(cmd2.pod_name.starts_with("helper-pod-create-"));
}

#[test]
fn validation_rejects_empty_relative_and_root_parent_paths() {
    let cfg = ProvisionerConfig::new(local(), 0);

    // empty name → error.
    let mut o = opts();
    o.name = String::new();
    assert!(matches!(
        build_helper_command(ActionType::Create, "helper-pod", &o, &cfg, false),
        Err(HelperError::EmptyNamePathOrNode)
    ));

    // relative path → error.
    let mut o = opts();
    o.path = "opt/lpp/vol".to_string();
    assert!(matches!(
        build_helper_command(ActionType::Create, "helper-pod", &o, &cfg, false),
        Err(HelperError::PathNotAbsolute { .. })
    ));

    // a path whose parent cleans to root ("/vol") → invalid (covers the `/` case).
    let mut o = opts();
    o.path = "/vol".to_string();
    assert!(matches!(
        build_helper_command(ActionType::Create, "helper-pod", &o, &cfg, false),
        Err(HelperError::InvalidVolumePath { .. })
    ));
}

#[test]
fn node_required_on_local_fs_but_optional_on_shared_fs() {
    let cfg = ProvisionerConfig::new(local(), 0);
    let mut o = opts();
    o.node = String::new();

    // local FS → node required.
    assert!(matches!(
        build_helper_command(ActionType::Create, "helper-pod", &o, &cfg, false),
        Err(HelperError::EmptyNamePathOrNode)
    ));

    // shared FS → empty node is fine; pod gets no node pin.
    let cmd = build_helper_command(ActionType::Create, "helper-pod", &o, &cfg, true).unwrap();
    assert_eq!(cmd.node, "");
}

#[test]
fn default_scripts_match_upstream_configmap() {
    assert!(DEFAULT_SETUP_SCRIPT.contains("mkdir -m 0777 -p \"$VOL_DIR\""));
    assert!(DEFAULT_TEARDOWN_SCRIPT.contains("rm -rf \"$VOL_DIR\""));
}
