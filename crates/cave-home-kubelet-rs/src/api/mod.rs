// SPDX-License-Identifier: Apache-2.0
//! Kubernetes API type subset.
//!
//! Hand-port of the relevant pieces of `k8s.io/api/core/v1/types.go` from
//! upstream `kubernetes/kubernetes` v1.36.1. Phase 1 deliberately stays away
//! from `k8s-openapi` / `kube-rs` (full type coverage is Phase 1b).
//!
//! Only the fields actually exercised by the kubelet sub-systems present in
//! Phase 1 are modelled. Anything the kubelet would consult later (probes,
//! resources, lifecycle hooks, security context) is recorded as
//! `[[unmapped]]`.

use std::collections::BTreeMap;

/// Pod UID — opaque, kubelet-side identifier of a pod.
///
/// Mirrors `k8s.io/apimachinery/pkg/types.UID`.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PodUid(pub String);

impl PodUid {
    /// Construct a new pod UID from any string-ish input.
    pub fn new<S: Into<String>>(uid: S) -> Self {
        Self(uid.into())
    }

    /// Borrow the underlying string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Container ID — `<runtime>://<id>` per `kubelet/container/runtime.go`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ContainerId(pub String);

impl ContainerId {
    pub fn new<S: Into<String>>(id: S) -> Self {
        Self(id.into())
    }
}

/// `ObjectMeta` subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectMeta {
    pub name: String,
    pub namespace: String,
    pub uid: PodUid,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
}

/// `OwnerReference` subset (used by the kubelet to pick a pod's controller
/// for status reporting).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OwnerReference {
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub uid: PodUid,
    pub controller: bool,
}

/// `EmptyDirVolumeSource` — `pkg/volume/emptydir`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmptyDirVolumeSource {
    /// Storage medium ("" for default = node disk, "Memory" = tmpfs).
    pub medium: String,
}

/// `HostPathType` — `core/v1/types.go`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HostPathType {
    /// `""` — no checks, the most permissive.
    #[default]
    Unset,
    /// `DirectoryOrCreate`.
    DirectoryOrCreate,
    /// `Directory` — must exist and be a directory.
    Directory,
    /// `FileOrCreate`.
    FileOrCreate,
    /// `File` — must exist and be a regular file.
    File,
    /// `Socket` — must be a unix socket.
    Socket,
    /// `CharDevice`.
    CharDevice,
    /// `BlockDevice`.
    BlockDevice,
}

/// `HostPathVolumeSource`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostPathVolumeSource {
    pub path: String,
    pub host_path_type: HostPathType,
}

/// Tagged union of the volume sources supported in Phase 1.
///
/// Anything else is recorded in `parity.manifest.toml` `[[unmapped]]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VolumeSource {
    EmptyDir(EmptyDirVolumeSource),
    HostPath(HostPathVolumeSource),
}

/// `Volume` (`core/v1/types.go`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Volume {
    pub name: String,
    pub source: VolumeSource,
}

/// `VolumeMount` — declared on a container.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VolumeMount {
    pub name: String,
    pub mount_path: String,
    pub read_only: bool,
}

/// Restart policy enum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RestartPolicy {
    #[default]
    Always,
    OnFailure,
    Never,
}

/// `Container` subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Container {
    pub name: String,
    pub image: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub volume_mounts: Vec<VolumeMount>,
}

/// `PodSpec` subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodSpec {
    pub containers: Vec<Container>,
    pub volumes: Vec<Volume>,
    pub restart_policy: RestartPolicy,
    pub node_name: String,
}

/// `Pod` — `metadata + spec + status`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Pod {
    pub metadata: ObjectMeta,
    pub spec: PodSpec,
    pub status: PodStatus,
}

impl Pod {
    /// Convenience: pod UID.
    #[must_use]
    pub fn uid(&self) -> &PodUid {
        &self.metadata.uid
    }

    /// Convenience: namespaced name `ns/name`.
    #[must_use]
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.metadata.namespace, self.metadata.name)
    }
}

/// `PodPhase` — high-level lifecycle phase.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PodPhase {
    #[default]
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

/// Container `Waiting` state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerStateWaiting {
    pub reason: String,
    pub message: String,
}

/// Container `Running` state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerStateRunning {
    /// Unix-millis when the container entered the running state.
    pub started_at_ms: u64,
}

/// Container `Terminated` state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerStateTerminated {
    pub exit_code: i32,
    pub reason: String,
    pub message: String,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
}

/// One-of `Waiting` / `Running` / `Terminated`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContainerState {
    Waiting(ContainerStateWaiting),
    Running(ContainerStateRunning),
    Terminated(ContainerStateTerminated),
}

impl Default for ContainerState {
    fn default() -> Self {
        Self::Waiting(ContainerStateWaiting::default())
    }
}

/// `ContainerStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerStatus {
    pub name: String,
    pub state: ContainerState,
    pub image: String,
    pub container_id: Option<ContainerId>,
    pub ready: bool,
    pub restart_count: i32,
}

/// `PodStatus` subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodStatus {
    pub phase: PodPhase,
    pub message: String,
    pub reason: String,
    pub container_statuses: Vec<ContainerStatus>,
}
