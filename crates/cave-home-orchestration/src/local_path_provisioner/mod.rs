//! `local_path_provisioner` — the in-process port of K3s's bundled storage
//! provisioner (rancher/local-path-provisioner v0.0.36, Apache-2.0).
//!
//! K3s ships `local-path-provisioner` as its default `StorageClass`: a dynamic
//! provisioner that, when a `PersistentVolumeClaim` binds, creates a `hostPath`
//! (or `local`) directory on the claim's selected node and hands back a
//! `PersistentVolume` pinned to that node. cave-home preserves the single-binary
//! shape (Charter §5, ADR-004): the provisioner is a **module** of
//! `cave-home-orchestration`, not a separate Deployment/pod — there is no
//! `local-path-storage` Deployment manifest and no per-crate Helm chart.
//!
//! This is a **behavioural port of the DECISION core** of the upstream
//! controller. The upstream `LocalPathProvisioner` interleaves two concerns:
//! the *decisions* (which node path to use, what folder name, what the helper
//! pod's command/env/args are, which volume source + node affinity the PV gets,
//! whether a `Delete` cleans up or retains) and the *I/O* (watching the K8s API,
//! creating helper pods, polling them, reading the config file). cave-home ports
//! the decision half here — pure, std-only, no clock, no network, no K8s client
//! — and defers the I/O half to ADR-004 phase-1b (enumerated in the parity
//! manifest `[[unmapped]]`). The split is the same one this crate already draws
//! for the control-plane bring-up: this module decides; a future supervisor
//! drives.
//!
//! Submodules:
//! - [`config`] — `nodePathMap` / `sharedFileSystemPath` canonicalization
//!   (lexical path clean + dedup, relative/root rejection), the shared-vs-local
//!   decision, and storage-class config pick.
//!
//! - [`path`] — `getPathOnNode` (node fallback, requested-path validation,
//!   deterministic candidate selection), the `pvName_namespace_pvcName` folder
//!   name, the `pathPattern` safe-prefix check, and the volume-path join.
//! - [`provision`] — the `Provision` decision: PVC validation (selector / access
//!   mode / node), the resulting [`provision::PvSpec`] (reclaim policy, capacity,
//!   node-affinity term, `hostPath`/`local` source), and the provisioning state.
//! - [`helper`] — the helper-pod *command*: action, command line, the
//!   `VOL_DIR`/`VOL_MODE`/`VOL_SIZE_BYTES` env, the `-p/-s/-m/-a` args, the
//!   `{base}-{action}-{name}` name truncation, and the default setup/teardown
//!   scripts.
//! - [`reclaim`] — the `Delete` decision: recover path + node from a PV's source
//!   and affinity, then retain-vs-teardown by reclaim policy.
//! - [`metrics`] — the observability descriptors (PV count by status,
//!   provisioning latency, reconcile error rate).
//!
//! Further submodules (`report`) land in subsequent TDD cycles.
//!
//! Like the rest of this crate it is **infrastructure**, hidden from end users
//! (Charter §6.3, ADR-007): no user-facing strings, no i18n.

pub mod config;
pub mod helper;
pub mod metrics;
pub mod path;
pub mod provision;
pub mod reclaim;

pub use config::ProvisionerConfig;

/// The node key under which an unlisted node falls back to a default path set
/// (upstream `NodeDefaultNonListedNodes`).
pub const NODE_DEFAULT_NON_LISTED_NODES: &str = "DEFAULT_PATH_FOR_NON_LISTED_NODES";

/// The node-affinity label key a provisioned PV is pinned to by default
/// (upstream `DefaultNodeAffinityKey`). A storage class may override it via the
/// `nodeAffinityKey` parameter.
pub const DEFAULT_NODE_AFFINITY_KEY: &str = "kubernetes.io/hostname";

/// The default volume source type (upstream `defaultVolumeType`).
pub const DEFAULT_VOLUME_TYPE: &str = "hostPath";

/// The default helper-pod command timeout in seconds (upstream
/// `defaultCmdTimeoutSeconds`), applied when the config sets `0`.
pub const DEFAULT_CMD_TIMEOUT_SECONDS: u32 = 120;

/// The maximum helper-pod name length; longer names are truncated (upstream
/// `HelperPodNameMaxLength`).
pub const HELPER_POD_NAME_MAX_LENGTH: usize = 128;
