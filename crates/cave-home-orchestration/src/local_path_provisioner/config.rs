//! Provisioner configuration canonicalization (port of the upstream
//! `canonicalizeStorageClassConfig` / `canonicalizeConfig` / `isSharedFilesystem`
//! / `pickConfig`).
//!
//! Upstream loads a JSON `ConfigData` and *canonicalizes* it into a `Config`:
//! each `nodePathMap` entry's paths are made absolute, cleaned, root-rejected
//! and de-duplicated; duplicate nodes are rejected; `cmdTimeoutSeconds` defaults
//! to 120. cave-home models the **canonicalized** form directly (the crate is
//! std-only, so it does not parse the JSON wire format — that is the loader's
//! job, ADR-004 phase-1b) and reproduces the same validation rules on the way
//! in, returning [`ConfigError`] instead of Go's `error` strings.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use super::{DEFAULT_CMD_TIMEOUT_SECONDS, NODE_DEFAULT_NON_LISTED_NODES};

/// The two helper-pod actions (upstream `ActionType`: `ActionTypeCreate`,
/// `ActionTypeDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActionType {
    /// Provision — create the volume directory.
    Create,
    /// Deprovision — remove the volume directory.
    Delete,
}

impl ActionType {
    /// The wire identifier used in the helper-pod name and the `-a` arg.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Delete => "delete",
        }
    }
}

impl fmt::Display for ActionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The set of absolute paths configured for one node (upstream `NodePathMap`,
/// whose `Paths` is a `map[string]struct{}`).
///
/// cave-home stores them in a [`BTreeSet`] so the candidate order is
/// deterministic — upstream selects a path with `rand.IntN`, which this crate
/// cannot reproduce (no RNG by design); deterministic ordering lets the caller
/// drive selection reproducibly (see [`super::path`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePathMap {
    paths: BTreeSet<String>,
}

impl NodePathMap {
    /// Canonicalize a node's raw path list.
    ///
    /// Reproduces upstream `canonicalizeStorageClassConfig`'s per-path loop:
    /// each path must begin with `/` ([`ConfigError::PathNotAbsolute`]), is
    /// lexically cleaned, must not clean to `/` ([`ConfigError::RootPath`]), and
    /// must be unique after cleaning ([`ConfigError::DuplicatePath`]). An empty
    /// list is valid (a node that refuses provisioning).
    ///
    /// # Errors
    /// See the variants named above.
    pub fn canonicalize(node: &str, paths: &[&str]) -> Result<Self, ConfigError> {
        let mut set = BTreeSet::new();
        for raw in paths {
            if !raw.starts_with('/') {
                return Err(ConfigError::PathNotAbsolute {
                    path: (*raw).to_string(),
                    node: node.to_string(),
                });
            }
            let cleaned = clean_abs_path(raw);
            if cleaned == "/" {
                return Err(ConfigError::RootPath {
                    node: node.to_string(),
                });
            }
            if !set.insert(cleaned.clone()) {
                return Err(ConfigError::DuplicatePath {
                    path: cleaned,
                    node: node.to_string(),
                });
            }
        }
        Ok(Self { paths: set })
    }

    /// The configured paths, sorted (deterministic candidate order).
    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        self.paths.iter().cloned().collect()
    }

    /// Whether a (cleaned) path is configured for this node.
    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        self.paths.contains(&clean_abs_path(path))
    }

    /// Whether this node has no configured path (provisioning is refused).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// The number of candidate paths.
    #[must_use]
    pub fn len(&self) -> usize {
        self.paths.len()
    }
}

/// One storage class's provisioning target (upstream `StorageClassConfig`):
/// either a per-node path map or a single shared-filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageClassConfig {
    node_path_map: BTreeMap<String, NodePathMap>,
    shared_filesystem_path: String,
}

impl StorageClassConfig {
    /// Canonicalize a storage class config from `(node, paths)` entries plus an
    /// optional shared-filesystem path.
    ///
    /// Reproduces upstream `canonicalizeStorageClassConfig`: duplicate nodes are
    /// rejected ([`ConfigError::DuplicateNode`]) and each node's paths are run
    /// through [`NodePathMap::canonicalize`]. The shared-vs-local *conflict* is
    /// not rejected here (upstream defers it to `isSharedFilesystem`); see
    /// [`Self::is_shared_filesystem`].
    ///
    /// # Errors
    /// [`ConfigError::DuplicateNode`] or any error from path canonicalization.
    pub fn canonicalize(
        node_paths: &[(&str, &[&str])],
        shared_filesystem_path: &str,
    ) -> Result<Self, ConfigError> {
        let mut map = BTreeMap::new();
        for (node, paths) in node_paths {
            let npm = NodePathMap::canonicalize(node, paths)?;
            if map.insert((*node).to_string(), npm).is_some() {
                return Err(ConfigError::DuplicateNode {
                    node: (*node).to_string(),
                });
            }
        }
        Ok(Self {
            node_path_map: map,
            shared_filesystem_path: shared_filesystem_path.to_string(),
        })
    }

    /// The shared-filesystem path, if this class uses one.
    #[must_use]
    pub fn shared_filesystem_path(&self) -> &str {
        &self.shared_filesystem_path
    }

    /// The path map for a node, if listed.
    #[must_use]
    pub fn node_path_map(&self, node: &str) -> Option<&NodePathMap> {
        self.node_path_map.get(node)
    }

    /// The path map for `node`, falling back to the
    /// [`NODE_DEFAULT_NON_LISTED_NODES`] default entry (upstream `getPathOnNode`
    /// fallback).
    #[must_use]
    pub fn node_path_map_or_default(&self, node: &str) -> Option<&NodePathMap> {
        self.node_path_map
            .get(node)
            .or_else(|| self.node_path_map.get(NODE_DEFAULT_NON_LISTED_NODES))
    }

    /// Whether this class provisions onto a shared filesystem (upstream
    /// `isSharedFilesystem`).
    ///
    /// Decision table: both configured → [`ConfigError::BothNodeMapAndSharedFs`];
    /// a non-empty node map → local (`false`); a non-empty shared path → shared
    /// (`true`); neither → [`ConfigError::NeitherNodeMapNorSharedFs`].
    ///
    /// # Errors
    /// The two variants named above, for the ambiguous / unconfigured cases.
    pub fn is_shared_filesystem(&self) -> Result<bool, ConfigError> {
        let has_shared = !self.shared_filesystem_path.is_empty();
        let has_map = !self.node_path_map.is_empty();
        if has_shared && has_map {
            return Err(ConfigError::BothNodeMapAndSharedFs);
        }
        if has_map {
            return Ok(false);
        }
        if has_shared {
            return Ok(true);
        }
        Err(ConfigError::NeitherNodeMapNorSharedFs)
    }
}

/// The whole provisioner config (upstream `Config`): a default storage class
/// config plus optionally a set of named per-storage-class configs, and the
/// helper command timeout / overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionerConfig {
    default_class: StorageClassConfig,
    storage_class_configs: BTreeMap<String, StorageClassConfig>,
    cmd_timeout_seconds: u32,
    setup_command: String,
    teardown_command: String,
}

impl ProvisionerConfig {
    /// A config with only the default (unnamed) storage class. `cmd_timeout` of
    /// `0` is canonicalized to [`DEFAULT_CMD_TIMEOUT_SECONDS`] (upstream
    /// `canonicalizeConfig`).
    #[must_use]
    pub const fn new(default_class: StorageClassConfig, cmd_timeout: u32) -> Self {
        Self {
            default_class,
            storage_class_configs: BTreeMap::new(),
            cmd_timeout_seconds: canonical_timeout(cmd_timeout),
            setup_command: String::new(),
            teardown_command: String::new(),
        }
    }

    /// A config with named per-storage-class configs (upstream
    /// `StorageClassConfigs`). When this map is non-empty the default class is
    /// never consulted; [`Self::pick`] requires an exact name match.
    #[must_use]
    pub fn with_classes(
        classes: impl IntoIterator<Item = (String, StorageClassConfig)>,
        cmd_timeout: u32,
    ) -> Self {
        Self {
            default_class: StorageClassConfig {
                node_path_map: BTreeMap::new(),
                shared_filesystem_path: String::new(),
            },
            storage_class_configs: classes.into_iter().collect(),
            cmd_timeout_seconds: canonical_timeout(cmd_timeout),
            setup_command: String::new(),
            teardown_command: String::new(),
        }
    }

    /// Override the custom setup/teardown commands (upstream `SetupCommand` /
    /// `TeardownCommand`; empty means "use the default `/bin/sh /script/...`").
    #[must_use]
    pub fn with_commands(mut self, setup: impl Into<String>, teardown: impl Into<String>) -> Self {
        self.setup_command = setup.into();
        self.teardown_command = teardown.into();
        self
    }

    /// Resolve the storage class config for a class name (upstream `pickConfig`).
    ///
    /// With no named configs, the default class is returned regardless of name;
    /// otherwise an exact match is required.
    ///
    /// # Errors
    /// [`ConfigError::UnknownStorageClass`] when named configs exist but none
    /// matches `storage_class_name`.
    pub fn pick(&self, storage_class_name: &str) -> Result<&StorageClassConfig, ConfigError> {
        if self.storage_class_configs.is_empty() {
            return Ok(&self.default_class);
        }
        self.storage_class_configs.get(storage_class_name).ok_or_else(|| {
            ConfigError::UnknownStorageClass {
                name: storage_class_name.to_string(),
            }
        })
    }

    /// The canonicalized helper-pod command timeout in seconds.
    #[must_use]
    pub const fn cmd_timeout_seconds(&self) -> u32 {
        self.cmd_timeout_seconds
    }

    /// The custom setup command, or `""` for the default script.
    #[must_use]
    pub fn setup_command(&self) -> &str {
        &self.setup_command
    }

    /// The custom teardown command, or `""` for the default script.
    #[must_use]
    pub fn teardown_command(&self) -> &str {
        &self.teardown_command
    }
}

/// Apply the upstream `cmdTimeoutSeconds > 0 ? : default` rule.
const fn canonical_timeout(raw: u32) -> u32 {
    if raw > 0 {
        raw
    } else {
        DEFAULT_CMD_TIMEOUT_SECONDS
    }
}

/// Lexically clean an absolute path (the `filepath.Abs`+`filepath.Clean` effect
/// for an already-absolute input): collapse `//`, drop `.`, resolve `..`
/// (clamped at root), and strip the trailing slash. Pure-lexical — it never
/// touches the filesystem.
fn clean_abs_path(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            seg => segments.push(seg),
        }
    }
    if segments.is_empty() {
        "/".to_string()
    } else {
        let mut out = String::with_capacity(path.len());
        for seg in &segments {
            out.push('/');
            out.push_str(seg);
        }
        out
    }
}

/// A canonicalization / lookup failure (upstream returns Go `error` strings;
/// cave-home returns typed variants, no panic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A node path did not begin with `/` (upstream "path must start with /").
    PathNotAbsolute {
        /// The offending raw path.
        path: String,
        /// The node it was configured on.
        node: String,
    },
    /// A node path cleaned to `/` (upstream "cannot use root ('/') as path").
    RootPath {
        /// The node it was configured on.
        node: String,
    },
    /// The same cleaned path appeared twice for one node (upstream "duplicate
    /// path").
    DuplicatePath {
        /// The duplicated cleaned path.
        path: String,
        /// The node it was configured on.
        node: String,
    },
    /// The same node appeared twice (upstream "duplicate node").
    DuplicateNode {
        /// The duplicated node name.
        node: String,
    },
    /// Both `nodePathMap` and `sharedFileSystemPath` were configured (upstream
    /// "both nodePathMap and sharedFileSystemPath are defined").
    BothNodeMapAndSharedFs,
    /// Neither `nodePathMap` nor `sharedFileSystemPath` was configured (upstream
    /// "both nodePathMap and sharedFileSystemPath are unconfigured").
    NeitherNodeMapNorSharedFs,
    /// A request named a storage class with no matching config (upstream "Got
    /// request for unexpected storage class").
    UnknownStorageClass {
        /// The unmatched storage class name.
        name: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathNotAbsolute { path, node } => {
                write!(f, "path must start with / for path {path} on node {node}")
            }
            Self::RootPath { node } => {
                write!(f, "cannot use root ('/') as path on node {node}")
            }
            Self::DuplicatePath { path, node } => {
                write!(f, "duplicate path {path} on node {node}")
            }
            Self::DuplicateNode { node } => write!(f, "duplicate node {node}"),
            Self::BothNodeMapAndSharedFs => f.write_str(
                "both nodePathMap and sharedFileSystemPath are defined; only one may be in use",
            ),
            Self::NeitherNodeMapNorSharedFs => {
                f.write_str("both nodePathMap and sharedFileSystemPath are unconfigured")
            }
            Self::UnknownStorageClass { name } => {
                write!(f, "request for unexpected storage class {name}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
