// SPDX-License-Identifier: Apache-2.0
//! CRI v1 type subset.
//!
//! Hand-port of the relevant slices of
//! `k8s.io/cri-api/pkg/apis/runtime/v1/api.proto` (v1.36.1).
//!
//! Only the fields actually consumed by Phase 1 sub-systems (PodWorker,
//! PLEG, VolumeManager, MockCriClient) are present. The rest is recorded as
//! `[[unmapped]]` Phase 1b.

use std::collections::BTreeMap;

/// `KeyValue` — protobuf message.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

/// `NamespaceMode` enum from `api.proto`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NamespaceMode {
    #[default]
    Pod,
    Container,
    Node,
    Target,
}

/// `NamespaceOption`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NamespaceOption {
    pub network: NamespaceMode,
    pub pid: NamespaceMode,
    pub ipc: NamespaceMode,
}

/// `Mount` — bind mount from host into container.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Mount {
    pub container_path: String,
    pub host_path: String,
    pub readonly: bool,
}

/// `ImageSpec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImageSpec {
    pub image: String,
}

/// `LinuxPodSandboxConfig` — minimal subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinuxPodSandboxConfig {
    pub cgroup_parent: String,
    pub namespace_options: NamespaceOption,
}

/// `PodSandboxMetadata`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodSandboxMetadata {
    pub name: String,
    pub uid: String,
    pub namespace: String,
    pub attempt: u32,
}

/// `PodSandboxConfig` — subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodSandboxConfig {
    pub metadata: PodSandboxMetadata,
    pub hostname: String,
    pub log_directory: String,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub linux: LinuxPodSandboxConfig,
}

/// `PodSandboxState`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PodSandboxState {
    #[default]
    NotReady,
    Ready,
}

/// `PodSandbox` — list item returned by `ListPodSandbox`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodSandbox {
    pub id: String,
    pub metadata: PodSandboxMetadata,
    pub state: PodSandboxState,
    pub created_at: i64,
    pub labels: BTreeMap<String, String>,
}

/// `PodSandboxStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodSandboxStatus {
    pub id: String,
    pub metadata: PodSandboxMetadata,
    pub state: PodSandboxState,
    pub created_at: i64,
}

/// `ContainerMetadata`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerMetadata {
    pub name: String,
    pub attempt: u32,
}

/// `ContainerConfig` — subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerConfig {
    pub metadata: ContainerMetadata,
    pub image: ImageSpec,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub envs: Vec<KeyValue>,
    pub mounts: Vec<Mount>,
    pub log_path: String,
    pub labels: BTreeMap<String, String>,
}

/// `ContainerState` per CRI proto.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ContainerState {
    #[default]
    Created,
    Running,
    Exited,
    Unknown,
}

/// `Container` — list item returned by `ListContainers`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Container {
    pub id: String,
    pub pod_sandbox_id: String,
    pub metadata: ContainerMetadata,
    pub image: ImageSpec,
    pub state: ContainerState,
    pub created_at: i64,
    pub labels: BTreeMap<String, String>,
}

/// `ContainerStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerStatus {
    pub id: String,
    pub metadata: ContainerMetadata,
    pub state: ContainerState,
    pub created_at: i64,
    pub started_at: i64,
    pub finished_at: i64,
    pub exit_code: i32,
    pub image: ImageSpec,
    pub reason: String,
    pub message: String,
}

/// `Image` (used by `image_status`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Image {
    pub id: String,
    pub repo_tags: Vec<String>,
    pub size_bytes: u64,
}

/// Filter passed to `ListContainers`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainerFilter {
    pub id: Option<String>,
    pub pod_sandbox_id: Option<String>,
    pub state: Option<ContainerState>,
}

/// Filter passed to `ListPodSandbox`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodSandboxFilter {
    pub id: Option<String>,
    pub state: Option<PodSandboxState>,
}
