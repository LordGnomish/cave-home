//! The `Provision` decision (port of the upstream `provisionFor`).
//!
//! Given a claim, its selected node and the storage class config, upstream
//! decides whether the claim is provisionable and, if so, builds the
//! `PersistentVolume`: it validates the claim (no label selector; only
//! `ReadWriteOnce` / `ReadWriteOncePod`; a node is required on a local
//! filesystem), resolves the volume path, picks the volume source type
//! (`hostPath` / `local`) from annotations, computes the node-affinity term, and
//! assembles the PV with the storage class's reclaim policy and the claim's
//! capacity. cave-home reproduces every one of those *decisions* as pure data
//! transforms; the helper-pod creation that upstream interleaves is the
//! [`super::helper`] cycle, and the actual K8s `PV` object emission is ADR-004
//! phase-1b.

use std::collections::BTreeMap;

use super::path::{base_path_on_node, folder_name, volume_path, PathError};
use super::{DEFAULT_NODE_AFFINITY_KEY, DEFAULT_VOLUME_TYPE};
use crate::local_path_provisioner::config::StorageClassConfig;

/// A persistent-volume access mode (the subset the provisioner reasons about).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    /// `ReadWriteOnce` — mountable read-write by a single node.
    ReadWriteOnce,
    /// `ReadWriteOncePod` — read-write by a single pod (Kubernetes 1.22+).
    ReadWriteOncePod,
    /// `ReadOnlyMany` — mountable read-only by many nodes.
    ReadOnlyMany,
    /// `ReadWriteMany` — mountable read-write by many nodes.
    ReadWriteMany,
}

impl AccessMode {
    /// The Kubernetes API short name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadWriteOnce => "ReadWriteOnce",
            Self::ReadWriteOncePod => "ReadWriteOncePod",
            Self::ReadOnlyMany => "ReadOnlyMany",
            Self::ReadWriteMany => "ReadWriteMany",
        }
    }

    /// Whether the provisioner supports this mode (upstream: only RWO/RWOP).
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self, Self::ReadWriteOnce | Self::ReadWriteOncePod)
    }
}

/// A persistent-volume reclaim policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimPolicy {
    /// Delete the backing directory when the PV is released (the LPP default).
    Delete,
    /// Keep the backing directory (manual reclaim).
    Retain,
    /// Recycle (deprecated upstream; modelled for completeness).
    Recycle,
}

impl ReclaimPolicy {
    /// The Kubernetes API name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "Delete",
            Self::Retain => "Retain",
            Self::Recycle => "Recycle",
        }
    }
}

/// A persistent-volume volume mode. A provisioned LPP PV is always
/// [`VolumeMode::Filesystem`] (upstream hardcodes `fs`); the claim's own mode is
/// only forwarded to the helper pod.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeMode {
    /// A filesystem-backed volume.
    Filesystem,
    /// A raw block volume.
    Block,
}

impl VolumeMode {
    /// The Kubernetes API name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Filesystem => "Filesystem",
            Self::Block => "Block",
        }
    }
}

/// The PV's volume source (upstream `createPersistentVolumeSource`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeSource {
    /// A `hostPath` volume (the default) with `DirectoryOrCreate` semantics.
    HostPath {
        /// The on-node directory path.
        path: String,
    },
    /// A `local` volume.
    Local {
        /// The on-node directory path.
        path: String,
    },
}

/// A node-affinity requirement term: `key In values` (upstream builds a single
/// `NodeSelectorRequirement` with the `In` operator).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAffinityTerm {
    /// The label key the PV is pinned to.
    pub key: String,
    /// The accepted values (a single node identity).
    pub values: Vec<String>,
}

/// The provisioning controller's return state (upstream
/// `pvController.ProvisioningState`). The LPP `provisionFor` path always returns
/// `Finished` (success or terminal error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisioningState {
    /// Provisioning is complete (or failed terminally).
    Finished,
    /// Provisioning continues asynchronously.
    InBackground,
    /// Nothing changed.
    NoChange,
    /// The claim should be rescheduled to another node.
    Reschedule,
}

/// The resolved `PersistentVolume` specification (the decision output; not a K8s
/// object — emission is ADR-004 phase-1b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PvSpec {
    /// The PV name (= the requested PV name).
    pub name: String,
    /// The `local.path.provisioner/selected-node` annotation value.
    pub selected_node_annotation: String,
    /// The reclaim policy (inherited from the storage class).
    pub reclaim_policy: ReclaimPolicy,
    /// The access modes (forwarded from the claim).
    pub access_modes: Vec<AccessMode>,
    /// The PV volume mode (always [`VolumeMode::Filesystem`]).
    pub volume_mode: VolumeMode,
    /// The capacity in bytes (from the claim's storage request).
    pub capacity_bytes: u64,
    /// The volume source (`hostPath` / `local`).
    pub source: VolumeSource,
    /// The node-affinity term, or `None` on a shared filesystem.
    pub node_affinity: Option<NodeAffinityTerm>,
}

/// The full outcome of a provision decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provisioned {
    /// The PV to create.
    pub pv: PvSpec,
    /// The on-node directory the helper pod must create.
    pub volume_path: String,
    /// The provisioning state to report to the controller.
    pub state: ProvisioningState,
}

/// The pure inputs to a provision decision (the relevant fields of the claim,
/// its selected node and the storage class).
#[derive(Debug, Clone)]
pub struct ProvisionRequest {
    pv_name: String,
    namespace: String,
    pvc_name: String,
    node_name: String,
    node_labels: BTreeMap<String, String>,
    has_selector: bool,
    access_modes: Vec<AccessMode>,
    capacity_bytes: u64,
    reclaim_policy: ReclaimPolicy,
    default_volume_type: Option<String>,
    pvc_volume_type: Option<String>,
    affinity_key_param: Option<String>,
    requested_path: String,
}

impl ProvisionRequest {
    /// A request with upstream-typical defaults: a single `ReadWriteOnce` mode,
    /// `Delete` reclaim, `hostPath` source, no selector, 0-byte capacity.
    #[must_use]
    pub fn new(
        pv_name: impl Into<String>,
        namespace: impl Into<String>,
        claim_name: impl Into<String>,
        node_name: impl Into<String>,
    ) -> Self {
        Self {
            pv_name: pv_name.into(),
            namespace: namespace.into(),
            pvc_name: claim_name.into(),
            node_name: node_name.into(),
            node_labels: BTreeMap::new(),
            has_selector: false,
            access_modes: vec![AccessMode::ReadWriteOnce],
            capacity_bytes: 0,
            reclaim_policy: ReclaimPolicy::Delete,
            default_volume_type: None,
            pvc_volume_type: None,
            affinity_key_param: None,
            requested_path: String::new(),
        }
    }

    /// Set the claim's label-selector presence (any selector is unsupported).
    #[must_use]
    pub const fn with_selector(mut self, present: bool) -> Self {
        self.has_selector = present;
        self
    }

    /// Set the claim's access modes.
    #[must_use]
    pub fn with_access_modes(mut self, modes: Vec<AccessMode>) -> Self {
        self.access_modes = modes;
        self
    }

    /// Set the requested capacity in bytes.
    #[must_use]
    pub const fn with_capacity_bytes(mut self, bytes: u64) -> Self {
        self.capacity_bytes = bytes;
        self
    }

    /// Set the storage class reclaim policy.
    #[must_use]
    pub const fn with_reclaim_policy(mut self, policy: ReclaimPolicy) -> Self {
        self.reclaim_policy = policy;
        self
    }

    /// Set the storage class `defaultVolumeType` annotation.
    #[must_use]
    pub fn with_default_volume_type(mut self, value: Option<String>) -> Self {
        self.default_volume_type = value;
        self
    }

    /// Set the claim `volumeType` annotation (overrides the SC default).
    #[must_use]
    pub fn with_pvc_volume_type(mut self, value: Option<String>) -> Self {
        self.pvc_volume_type = value;
        self
    }

    /// Set the storage class `nodeAffinityKey` parameter.
    #[must_use]
    pub fn with_affinity_key_param(mut self, value: Option<String>) -> Self {
        self.affinity_key_param = value;
        self
    }

    /// Set the storage class `nodePath` parameter (a specific requested path).
    #[must_use]
    pub fn with_requested_path(mut self, path: impl Into<String>) -> Self {
        self.requested_path = path.into();
        self
    }

    /// Add a label to the selected node.
    #[must_use]
    pub fn with_node_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.node_labels.insert(key.into(), value.into());
        self
    }
}

/// Decide how to provision a claim (upstream `provisionFor`).
///
/// `selector` chooses among a node's candidate base paths (see
/// [`base_path_on_node`]). On success returns the [`Provisioned`] outcome with a
/// [`ProvisioningState::Finished`] state.
///
/// # Errors
/// [`ProvisionError`] for any validation failure (selector, access mode, missing
/// node, unrecognized volume type) or path-resolution failure.
pub fn decide_provision(
    req: &ProvisionRequest,
    cfg: &StorageClassConfig,
    selector: usize,
) -> Result<Provisioned, ProvisionError> {
    let shared_fs = cfg.is_shared_filesystem().map_err(PathError::Config)?;

    // Claim validation only applies on a local filesystem (upstream skips it for
    // shared FS, where any node can mount the path).
    if !shared_fs {
        if req.has_selector {
            return Err(ProvisionError::SelectorNotSupported);
        }
        for mode in &req.access_modes {
            if !mode.is_supported() {
                return Err(ProvisionError::UnsupportedAccessMode { mode: *mode });
            }
        }
        if req.node_name.is_empty() {
            return Err(ProvisionError::NoNodeSpecified);
        }
    }

    // On shared FS the node is ignored in path resolution (upstream passes "").
    let node_for_path = if shared_fs { "" } else { req.node_name.as_str() };
    let base = base_path_on_node(cfg, node_for_path, &req.requested_path, selector)?;
    let folder = folder_name(&req.pv_name, &req.namespace, &req.pvc_name);
    let path = volume_path(&base, &folder);

    let volume_type = resolve_volume_type(req);
    let source = make_volume_source(&volume_type, &path)?;

    let node_affinity = if shared_fs {
        None
    } else {
        Some(make_node_affinity(req))
    };

    let pv = PvSpec {
        name: req.pv_name.clone(),
        selected_node_annotation: req.node_name.clone(),
        reclaim_policy: req.reclaim_policy,
        access_modes: req.access_modes.clone(),
        volume_mode: VolumeMode::Filesystem,
        capacity_bytes: req.capacity_bytes,
        source,
        node_affinity,
    };

    Ok(Provisioned {
        pv,
        volume_path: path,
        state: ProvisioningState::Finished,
    })
}

/// Resolve the effective volume type (upstream annotation precedence: SC
/// `defaultVolumeType` annotation, then the `defaultVolumeType` constant, then
/// the claim's `volumeType` annotation wins outright).
fn resolve_volume_type(req: &ProvisionRequest) -> String {
    // The claim's volumeType annotation wins outright; otherwise the storage
    // class defaultVolumeType annotation; otherwise the hostPath default.
    if let Some(pvc_type) = &req.pvc_volume_type {
        return pvc_type.clone();
    }
    req.default_volume_type
        .clone()
        .unwrap_or_else(|| DEFAULT_VOLUME_TYPE.to_string())
}

/// Build the volume source for a (case-insensitive) type name (upstream
/// `createPersistentVolumeSource`).
fn make_volume_source(volume_type: &str, path: &str) -> Result<VolumeSource, ProvisionError> {
    match volume_type.to_ascii_lowercase().as_str() {
        "local" => Ok(VolumeSource::Local {
            path: path.to_string(),
        }),
        "hostpath" => Ok(VolumeSource::HostPath {
            path: path.to_string(),
        }),
        _ => Err(ProvisionError::UnrecognizedVolumeType {
            name: volume_type.to_string(),
        }),
    }
}

/// Build the node-affinity term (upstream: key from `nodeAffinityKey` param or
/// `DefaultNodeAffinityKey`; value from the node's label under that key, falling
/// back to the node name).
fn make_node_affinity(req: &ProvisionRequest) -> NodeAffinityTerm {
    let key = req
        .affinity_key_param
        .clone()
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| DEFAULT_NODE_AFFINITY_KEY.to_string());
    let value = req
        .node_labels
        .get(&key)
        .cloned()
        .unwrap_or_else(|| req.node_name.clone());
    NodeAffinityTerm {
        key,
        values: vec![value],
    }
}

/// A provision-decision failure (upstream returns terminal `error`s with
/// `ProvisioningFinished`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisionError {
    /// The claim carries a label selector (upstream "claim.Spec.Selector is not
    /// supported").
    SelectorNotSupported,
    /// The claim requested an unsupported access mode (upstream "`NodePath` only
    /// supports `ReadWriteOnce` and `ReadWriteOncePod`").
    UnsupportedAccessMode {
        /// The unsupported mode.
        mode: AccessMode,
    },
    /// No node was specified for a local-filesystem claim (upstream
    /// "configuration error, no node was specified").
    NoNodeSpecified,
    /// The resolved volume type is not `hostPath` / `local` (upstream "is not a
    /// recognised volume type").
    UnrecognizedVolumeType {
        /// The unrecognized type name.
        name: String,
    },
    /// Base-path resolution failed.
    Path(PathError),
}

impl From<PathError> for ProvisionError {
    fn from(e: PathError) -> Self {
        Self::Path(e)
    }
}

impl core::fmt::Display for ProvisionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SelectorNotSupported => f.write_str("claim.Spec.Selector is not supported"),
            Self::UnsupportedAccessMode { mode } => write!(
                f,
                "NodePath only supports ReadWriteOnce and ReadWriteOncePod access modes, got {}",
                mode.as_str()
            ),
            Self::NoNodeSpecified => f.write_str("configuration error, no node was specified"),
            Self::UnrecognizedVolumeType { name } => {
                write!(f, "\"{name}\" is not a recognised volume type")
            }
            Self::Path(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ProvisionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Path(e) => Some(e),
            _ => None,
        }
    }
}
