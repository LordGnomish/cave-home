//! The helper-pod **command** decision (port of the pure parts of the upstream
//! `createHelperPod`).
//!
//! Upstream provisions/cleans a volume directory by launching a short-lived
//! "helper pod" on the volume's node that runs a setup (`mkdir`) or teardown
//! (`rm -rf`) script. `createHelperPod` mixes the *command construction* — the
//! container command, the `VOL_DIR`/`VOL_MODE`/`VOL_SIZE_BYTES` env, the
//! `-p/-s/-m/-a` args, the `{base}-{action}-{name}` pod name, and the
//! parent/volume-dir split validation — with the *I/O* of creating the pod and
//! polling it to completion. cave-home ports the command construction here (pure
//! data); the pod create/poll loop is ADR-004 phase-1b.

use super::config::ActionType;
use super::path::clean_abs;
use super::{ProvisionerConfig, HELPER_POD_NAME_MAX_LENGTH};
use crate::local_path_provisioner::provision::VolumeMode;

/// The helper-pod script mount directory (upstream `helperScriptDir`).
pub const HELPER_SCRIPT_DIR: &str = "/script";

/// The `VOL_DIR` env var name (upstream `envVolDir`).
pub const ENV_VOL_DIR: &str = "VOL_DIR";
/// The `VOL_MODE` env var name (upstream `envVolMode`).
pub const ENV_VOL_MODE: &str = "VOL_MODE";
/// The `VOL_SIZE_BYTES` env var name (upstream `envVolSize`).
pub const ENV_VOL_SIZE: &str = "VOL_SIZE_BYTES";

/// The default setup script (upstream configmap `setup`): create the volume
/// directory world-writable.
pub const DEFAULT_SETUP_SCRIPT: &str = "#!/bin/sh\nset -eu\nmkdir -m 0777 -p \"$VOL_DIR\"\n";
/// The default teardown script (upstream configmap `teardown`): remove the
/// volume directory.
pub const DEFAULT_TEARDOWN_SCRIPT: &str = "#!/bin/sh\nset -eu\nrm -rf \"$VOL_DIR\"\n";

/// The inputs to one helper-pod invocation (upstream `volumeOptions`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeOptions {
    /// The PV name.
    pub name: String,
    /// The absolute on-node volume path.
    pub path: String,
    /// The volume mode forwarded from the claim.
    pub mode: VolumeMode,
    /// The requested size in bytes.
    pub size_bytes: u64,
    /// The node to pin the pod to (empty on a shared filesystem).
    pub node: String,
}

/// The resolved helper-pod command (the decision output; not a K8s Pod object).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperCommand {
    /// The helper pod name (`{base}-{action}-{name}`, truncated to 128 bytes).
    pub pod_name: String,
    /// The action this pod performs.
    pub action: ActionType,
    /// The container command (default `/bin/sh /script/{setup,teardown}` or the
    /// configured custom command).
    pub command: Vec<String>,
    /// The container environment, as ordered `(name, value)` pairs.
    pub env: Vec<(String, String)>,
    /// The container args (`-p <path> -s <size> -m <mode> -a <action>`).
    pub args: Vec<String>,
    /// The node to pin the pod to (empty when unpinned / shared FS).
    pub node: String,
}

impl HelperCommand {
    /// Look up an env var value by name.
    #[must_use]
    pub fn env_var(&self, name: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Build the helper-pod command for an action (upstream `createHelperPod`'s pure
/// construction).
///
/// `shared_fs` selects whether a node is required (it is not on a shared
/// filesystem). `base_pod_name` is the helper pod template's base name.
///
/// # Errors
/// [`HelperError::EmptyNamePathOrNode`] when the name/path is empty (or the node
/// is empty on a local FS), [`HelperError::PathNotAbsolute`] when the path is
/// not absolute, or [`HelperError::InvalidVolumePath`] when the path cannot be
/// split into a non-empty absolute parent directory and a volume directory.
pub fn build_helper_command(
    action: ActionType,
    base_pod_name: &str,
    opts: &VolumeOptions,
    cfg: &ProvisionerConfig,
    shared_fs: bool,
) -> Result<HelperCommand, HelperError> {
    if opts.name.is_empty() || opts.path.is_empty() || (!shared_fs && opts.node.is_empty()) {
        return Err(HelperError::EmptyNamePathOrNode);
    }
    if !opts.path.starts_with('/') {
        return Err(HelperError::PathNotAbsolute {
            path: opts.path.clone(),
        });
    }

    let path = clean_abs(&opts.path);
    let (parent_dir, volume_dir) = split_parent_volume(&path);
    if parent_dir.is_empty() || volume_dir.is_empty() || !parent_dir.starts_with('/') {
        return Err(HelperError::InvalidVolumePath { path });
    }
    // VOL_DIR = filepath.Join(parentDir, volumeDir) — i.e. the cleaned path.
    let vol_dir = format!("{parent_dir}/{volume_dir}");

    let command = resolve_command(action, cfg);
    let size = opts.size_bytes.to_string();
    let mode = opts.mode.as_str().to_string();

    let env = vec![
        (ENV_VOL_DIR.to_string(), vol_dir.clone()),
        (ENV_VOL_MODE.to_string(), mode.clone()),
        (ENV_VOL_SIZE.to_string(), size.clone()),
    ];
    let args = vec![
        "-p".to_string(),
        vol_dir,
        "-s".to_string(),
        size,
        "-m".to_string(),
        mode,
        "-a".to_string(),
        action.as_str().to_string(),
    ];

    let pod_name = truncate_bytes(
        &format!("{base_pod_name}-{}-{}", action.as_str(), opts.name),
        HELPER_POD_NAME_MAX_LENGTH,
    );
    // On a shared FS the pod is not pinned to a node.
    let node = if shared_fs {
        String::new()
    } else {
        opts.node.clone()
    };

    Ok(HelperCommand {
        pod_name,
        action,
        command,
        env,
        args,
        node,
    })
}

/// Choose the container command for an action: the configured custom command if
/// non-empty, else the default `/bin/sh /script/{setup,teardown}`.
fn resolve_command(action: ActionType, cfg: &ProvisionerConfig) -> Vec<String> {
    let (custom, script) = match action {
        ActionType::Create => (cfg.setup_command(), "setup"),
        ActionType::Delete => (cfg.teardown_command(), "teardown"),
    };
    if custom.is_empty() {
        vec![
            "/bin/sh".to_string(),
            format!("{HELPER_SCRIPT_DIR}/{script}"),
        ]
    } else {
        vec![custom.to_string()]
    }
}

/// Split a cleaned absolute path into `(parent_dir, volume_dir)` with trailing
/// separators trimmed (upstream `filepath.Split` + the `TrimSuffix` dance).
fn split_parent_volume(path: &str) -> (String, String) {
    path.rfind('/').map_or_else(
        || (String::new(), path.to_string()),
        |idx| {
            let parent = path[..idx].to_string(); // slash dropped (= TrimSuffix)
            let volume = path[idx + 1..].trim_end_matches('/').to_string();
            (parent, volume)
        },
    )
}

/// Truncate a string to at most `max` bytes, respecting char boundaries.
fn truncate_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// A helper-command construction failure (upstream returns Go `error` strings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperError {
    /// The name or path was empty, or the node was empty on a local filesystem
    /// (upstream "invalid empty name or path or node").
    EmptyNamePathOrNode,
    /// The volume path was not absolute (upstream "volume path is not
    /// absolute").
    PathNotAbsolute {
        /// The offending path.
        path: String,
    },
    /// The path could not be split into an absolute parent dir + volume dir
    /// (upstream "invalid path … cannot find parent dir or volume dir …").
    InvalidVolumePath {
        /// The offending cleaned path.
        path: String,
    },
}

impl core::fmt::Display for HelperError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyNamePathOrNode => f.write_str("invalid empty name or path or node"),
            Self::PathNotAbsolute { path } => write!(f, "volume path {path} is not absolute"),
            Self::InvalidVolumePath { path } => write!(
                f,
                "invalid path {path}: cannot find parent dir or volume dir or parent dir is relative"
            ),
        }
    }
}

impl std::error::Error for HelperError {}
