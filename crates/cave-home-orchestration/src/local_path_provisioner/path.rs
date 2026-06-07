//! Path selection (port of the upstream `getPathOnNode`, the folder-name join,
//! `filepath.Join` for the volume path, and `pathFromPattern`'s safe-prefix /
//! `filepath.IsLocal` guard).
//!
//! Upstream `getPathOnNode` chooses the base directory a volume is created
//! under: it short-circuits to the shared-filesystem path, otherwise looks up
//! the node's path map (falling back to the
//! `DEFAULT_PATH_FOR_NON_LISTED_NODES` entry), rejects an empty path set, honours
//! a storage-class-requested path, and otherwise picks one at random
//! (`rand.IntN`). cave-home reproduces every branch except the RNG: path
//! selection is **externalized** to a caller-supplied `selector` index into the
//! deterministically-sorted candidate list, so the decision is reproducible
//! (this crate carries no RNG by design).

use super::config::{ConfigError, StorageClassConfig};

/// Join a PV name, namespace and PVC name into the default folder name
/// (upstream `strings.Join([]string{name, namespace, pvcName}, "_")`).
#[must_use]
pub fn folder_name(pv_name: &str, namespace: &str, claim_name: &str) -> String {
    format!("{pv_name}_{namespace}_{claim_name}")
}

/// Resolve the base directory for a volume on `node` (upstream `getPathOnNode`).
///
/// - shared filesystem → the shared path, ignoring `node` and `selector`;
/// - otherwise the node's path map (or the `DEFAULT_PATH_FOR_NON_LISTED_NODES`
///   fallback) must exist and be non-empty;
/// - a non-empty `requested_path` must be one of the node's configured paths
///   (compared after lexical cleaning) and is returned as-is;
/// - otherwise `selector` indexes (modulo length) into the sorted candidate
///   paths — the deterministic stand-in for upstream `rand.IntN`.
///
/// # Errors
/// [`PathError::NoNodeConfigured`], [`PathError::NoLocalPath`],
/// [`PathError::RequestedPathNotConfigured`], or [`PathError::Config`] when the
/// shared-vs-local decision itself is ambiguous/unconfigured.
pub fn base_path_on_node(
    cfg: &StorageClassConfig,
    node: &str,
    requested_path: &str,
    selector: usize,
) -> Result<String, PathError> {
    if cfg.is_shared_filesystem().map_err(PathError::Config)? {
        // shared FS: node is ignored, the single shared path is used.
        return Ok(cfg.shared_filesystem_path().to_string());
    }

    let npm = cfg
        .node_path_map_or_default(node)
        .ok_or_else(|| PathError::NoNodeConfigured {
            node: node.to_string(),
        })?;
    let candidates = npm.paths();
    if candidates.is_empty() {
        return Err(PathError::NoLocalPath {
            node: node.to_string(),
        });
    }

    if !requested_path.is_empty() {
        if npm.contains(requested_path) {
            // Return the cleaned canonical form (the configured key).
            return Ok(clean_abs(requested_path));
        }
        return Err(PathError::RequestedPathNotConfigured {
            path: requested_path.to_string(),
            node: node.to_string(),
        });
    }

    // Deterministic pick: selector mod len (upstream: rand.IntN(len)).
    let idx = selector % candidates.len();
    Ok(candidates[idx].clone())
}

/// Join a base path and a folder name into the absolute volume path, cleaned
/// (upstream `filepath.Join(basePath, folderName)`).
#[must_use]
pub fn volume_path(base_path: &str, folder_name: &str) -> String {
    let joined = format!("{}/{folder_name}", base_path.trim_end_matches('/'));
    clean_abs(&joined)
}

/// Validate a rendered `pathPattern` result (upstream `pathFromPattern` plus the
/// `filepath.IsLocal` guard in `provisionFor`).
///
/// When `allow_unsafe` is set the path is accepted verbatim. Otherwise it must
/// begin with the `"<namespace>/<pvc_name>/"` prefix **and** be filesystem-local
/// (no rooted path, no net escape above its own subtree). Returns the validated
/// path on success.
///
/// # Errors
/// [`PathError::UnsafePathPattern`] when either guard fails.
pub fn validate_pattern_path(
    rendered: &str,
    namespace: &str,
    pvc_name: &str,
    allow_unsafe: bool,
) -> Result<String, PathError> {
    if allow_unsafe {
        return Ok(rendered.to_string());
    }
    let required_prefix = format!("{namespace}/{pvc_name}/");
    if !rendered.starts_with(&required_prefix) {
        return Err(PathError::UnsafePathPattern {
            path: rendered.to_string(),
        });
    }
    if !is_local(rendered) {
        return Err(PathError::UnsafePathPattern {
            path: rendered.to_string(),
        });
    }
    Ok(rendered.to_string())
}

/// Port of `filepath.IsLocal`: a path is local if it is non-empty, not rooted,
/// and — after lexical cleaning — does not escape the subtree it names (its
/// cleaned form does not begin with a `..` element).
#[must_use]
pub fn is_local(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') {
        return false;
    }
    let cleaned = clean_rel(path);
    cleaned != ".." && !cleaned.starts_with("../")
}

/// Lexically clean an absolute path (collapse `//`, drop `.`, resolve `..`
/// clamped at root, strip trailing slash). Pure-lexical; never touches the FS.
pub(crate) fn clean_abs(path: &str) -> String {
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

/// Lexically clean a *relative* path, preserving leading `..` elements that
/// escape the starting directory (the signal [`is_local`] keys on).
fn clean_rel(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if matches!(segments.last(), Some(&s) if s != "..") {
                    segments.pop();
                } else {
                    segments.push("..");
                }
            }
            seg => segments.push(seg),
        }
    }
    if segments.is_empty() {
        ".".to_string()
    } else {
        segments.join("/")
    }
}

/// A path-selection failure (upstream returns Go `error` strings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// The node is not in the path map and there is no
    /// `DEFAULT_PATH_FOR_NON_LISTED_NODES` fallback (upstream "config doesn't
    /// contain node X, and no default available").
    NoNodeConfigured {
        /// The unconfigured node.
        node: String,
    },
    /// The node has an empty path set (upstream "no local path available on
    /// node X").
    NoLocalPath {
        /// The node with no candidate paths.
        node: String,
    },
    /// A storage-class-requested path is not among the node's configured paths
    /// (upstream "config doesn't contain path X on node Y").
    RequestedPathNotConfigured {
        /// The requested path.
        path: String,
        /// The node it was requested on.
        node: String,
    },
    /// A `pathPattern` result failed the safe-prefix / locality guard (upstream
    /// "pathPattern must start with …" / "folder path contains invalid
    /// references").
    UnsafePathPattern {
        /// The offending rendered path.
        path: String,
    },
    /// The shared-vs-local decision itself was ambiguous or unconfigured.
    Config(ConfigError),
}

impl core::fmt::Display for PathError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoNodeConfigured { node } => {
                write!(f, "config doesn't contain node {node}, and no default available")
            }
            Self::NoLocalPath { node } => write!(f, "no local path available on node {node}"),
            Self::RequestedPathNotConfigured { path, node } => {
                write!(f, "config doesn't contain path {path} on node {node}")
            }
            Self::UnsafePathPattern { path } => {
                write!(f, "folder path contains invalid references: {path}")
            }
            Self::Config(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(e) => Some(e),
            _ => None,
        }
    }
}
