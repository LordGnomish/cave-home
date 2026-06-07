// SPDX-License-Identifier: Apache-2.0
//! Marshalling between the hand-ported CRI subset (`crate::cri::types`) and the
//! generated `runtime.v1` wire types (`super::proto`).
//!
//! These `From` impls are the cave-home analogue of the converters the kubelet
//! keeps in `pkg/kubelet/kuberuntime` — they translate the decision core's
//! structs onto the protobuf the runtime understands and back. Field numbers
//! and enum meanings follow `proto/api.proto` exactly; in particular the
//! sandbox-state enum is mapped by *meaning* (`READY = 0`, `NOTREADY = 1`),
//! which is the reverse of the native enum's declaration order.

use std::collections::{BTreeMap, HashMap};

use super::proto;
use crate::cri::types as t;

// ---------------------------------------------------------------------------
// maps
// ---------------------------------------------------------------------------

fn map_to_proto(m: BTreeMap<String, String>) -> HashMap<String, String> {
    m.into_iter().collect()
}

fn map_from_proto(m: HashMap<String, String>) -> BTreeMap<String, String> {
    m.into_iter().collect()
}

// ---------------------------------------------------------------------------
// enums
// ---------------------------------------------------------------------------

impl From<t::NamespaceMode> for proto::NamespaceMode {
    fn from(m: t::NamespaceMode) -> Self {
        match m {
            t::NamespaceMode::Pod => Self::Pod,
            t::NamespaceMode::Container => Self::Container,
            t::NamespaceMode::Node => Self::Node,
            t::NamespaceMode::Target => Self::Target,
        }
    }
}

impl From<t::PodSandboxState> for proto::PodSandboxState {
    fn from(s: t::PodSandboxState) -> Self {
        match s {
            t::PodSandboxState::Ready => Self::SandboxReady,
            t::PodSandboxState::NotReady => Self::SandboxNotready,
        }
    }
}

impl From<proto::PodSandboxState> for t::PodSandboxState {
    fn from(s: proto::PodSandboxState) -> Self {
        match s {
            proto::PodSandboxState::SandboxReady => Self::Ready,
            proto::PodSandboxState::SandboxNotready => Self::NotReady,
        }
    }
}

impl From<t::ContainerState> for proto::ContainerState {
    fn from(s: t::ContainerState) -> Self {
        match s {
            t::ContainerState::Created => Self::ContainerCreated,
            t::ContainerState::Running => Self::ContainerRunning,
            t::ContainerState::Exited => Self::ContainerExited,
            t::ContainerState::Unknown => Self::ContainerUnknown,
        }
    }
}

impl From<proto::ContainerState> for t::ContainerState {
    fn from(s: proto::ContainerState) -> Self {
        match s {
            proto::ContainerState::ContainerCreated => Self::Created,
            proto::ContainerState::ContainerRunning => Self::Running,
            proto::ContainerState::ContainerExited => Self::Exited,
            proto::ContainerState::ContainerUnknown => Self::Unknown,
        }
    }
}

/// Decode a wire `i32` sandbox-state, defaulting unknown values to `NotReady`
/// (the safe "not serving" interpretation, matching kubelet behaviour).
fn sandbox_state_from_i32(v: i32) -> t::PodSandboxState {
    proto::PodSandboxState::try_from(v)
        .unwrap_or(proto::PodSandboxState::SandboxNotready)
        .into()
}

/// Decode a wire `i32` container-state, defaulting unknown values to `Unknown`.
fn container_state_from_i32(v: i32) -> t::ContainerState {
    proto::ContainerState::try_from(v)
        .unwrap_or(proto::ContainerState::ContainerUnknown)
        .into()
}

// ---------------------------------------------------------------------------
// metadata
// ---------------------------------------------------------------------------

impl From<t::PodSandboxMetadata> for proto::PodSandboxMetadata {
    fn from(m: t::PodSandboxMetadata) -> Self {
        Self {
            name: m.name,
            uid: m.uid,
            namespace: m.namespace,
            attempt: m.attempt,
        }
    }
}

impl From<proto::PodSandboxMetadata> for t::PodSandboxMetadata {
    fn from(m: proto::PodSandboxMetadata) -> Self {
        Self {
            name: m.name,
            uid: m.uid,
            namespace: m.namespace,
            attempt: m.attempt,
        }
    }
}

impl From<t::ContainerMetadata> for proto::ContainerMetadata {
    fn from(m: t::ContainerMetadata) -> Self {
        Self {
            name: m.name,
            attempt: m.attempt,
        }
    }
}

impl From<proto::ContainerMetadata> for t::ContainerMetadata {
    fn from(m: proto::ContainerMetadata) -> Self {
        Self {
            name: m.name,
            attempt: m.attempt,
        }
    }
}

// ---------------------------------------------------------------------------
// image
// ---------------------------------------------------------------------------

impl From<t::ImageSpec> for proto::ImageSpec {
    fn from(s: t::ImageSpec) -> Self {
        Self {
            image: s.image,
            ..Self::default()
        }
    }
}

impl From<proto::ImageSpec> for t::ImageSpec {
    fn from(s: proto::ImageSpec) -> Self {
        Self { image: s.image }
    }
}

impl From<proto::Image> for t::Image {
    fn from(i: proto::Image) -> Self {
        Self {
            id: i.id,
            repo_tags: i.repo_tags,
            size_bytes: i.size,
        }
    }
}

// ---------------------------------------------------------------------------
// pod sandbox config (native -> proto only; the kubelet never receives one)
// ---------------------------------------------------------------------------

impl From<t::NamespaceOption> for proto::NamespaceOption {
    fn from(n: t::NamespaceOption) -> Self {
        Self {
            network: proto::NamespaceMode::from(n.network) as i32,
            pid: proto::NamespaceMode::from(n.pid) as i32,
            ipc: proto::NamespaceMode::from(n.ipc) as i32,
            ..Self::default()
        }
    }
}

impl From<t::LinuxPodSandboxConfig> for proto::LinuxPodSandboxConfig {
    fn from(l: t::LinuxPodSandboxConfig) -> Self {
        Self {
            cgroup_parent: l.cgroup_parent,
            // The native model carries namespace options at the top level; on
            // the wire they live under the sandbox security context.
            security_context: Some(proto::LinuxSandboxSecurityContext {
                namespace_options: Some(l.namespace_options.into()),
                ..proto::LinuxSandboxSecurityContext::default()
            }),
            ..Self::default()
        }
    }
}

impl From<t::PodSandboxConfig> for proto::PodSandboxConfig {
    fn from(c: t::PodSandboxConfig) -> Self {
        Self {
            metadata: Some(c.metadata.into()),
            hostname: c.hostname,
            log_directory: c.log_directory,
            labels: map_to_proto(c.labels),
            annotations: map_to_proto(c.annotations),
            linux: Some(c.linux.into()),
            ..Self::default()
        }
    }
}

// ---------------------------------------------------------------------------
// pod sandbox (proto -> native)
// ---------------------------------------------------------------------------

impl From<proto::PodSandbox> for t::PodSandbox {
    fn from(p: proto::PodSandbox) -> Self {
        Self {
            id: p.id,
            metadata: p.metadata.map(Into::into).unwrap_or_default(),
            state: sandbox_state_from_i32(p.state),
            created_at: p.created_at,
            labels: map_from_proto(p.labels),
        }
    }
}

impl From<proto::PodSandboxStatus> for t::PodSandboxStatus {
    fn from(p: proto::PodSandboxStatus) -> Self {
        Self {
            id: p.id,
            metadata: p.metadata.map(Into::into).unwrap_or_default(),
            state: sandbox_state_from_i32(p.state),
            created_at: p.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// container config (native -> proto)
// ---------------------------------------------------------------------------

impl From<t::Mount> for proto::Mount {
    fn from(m: t::Mount) -> Self {
        Self {
            container_path: m.container_path,
            host_path: m.host_path,
            readonly: m.readonly,
            ..Self::default()
        }
    }
}

impl From<t::KeyValue> for proto::KeyValue {
    fn from(kv: t::KeyValue) -> Self {
        Self {
            key: kv.key,
            value: kv.value,
        }
    }
}

impl From<t::ContainerConfig> for proto::ContainerConfig {
    fn from(c: t::ContainerConfig) -> Self {
        Self {
            metadata: Some(c.metadata.into()),
            image: Some(c.image.into()),
            command: c.command,
            args: c.args,
            envs: c.envs.into_iter().map(Into::into).collect(),
            mounts: c.mounts.into_iter().map(Into::into).collect(),
            log_path: c.log_path,
            labels: map_to_proto(c.labels),
            ..Self::default()
        }
    }
}

// ---------------------------------------------------------------------------
// container / container status (proto -> native)
// ---------------------------------------------------------------------------

impl From<proto::Container> for t::Container {
    fn from(c: proto::Container) -> Self {
        Self {
            id: c.id,
            pod_sandbox_id: c.pod_sandbox_id,
            metadata: c.metadata.map(Into::into).unwrap_or_default(),
            image: c.image.map(Into::into).unwrap_or_default(),
            state: container_state_from_i32(c.state),
            created_at: c.created_at,
            labels: map_from_proto(c.labels),
        }
    }
}

impl From<proto::ContainerStatus> for t::ContainerStatus {
    fn from(c: proto::ContainerStatus) -> Self {
        Self {
            id: c.id,
            metadata: c.metadata.map(Into::into).unwrap_or_default(),
            state: container_state_from_i32(c.state),
            created_at: c.created_at,
            started_at: c.started_at,
            finished_at: c.finished_at,
            exit_code: c.exit_code,
            image: c.image.map(Into::into).unwrap_or_default(),
            reason: c.reason,
            message: c.message,
        }
    }
}

// ---------------------------------------------------------------------------
// filters (native -> proto)
// ---------------------------------------------------------------------------

impl From<t::PodSandboxFilter> for proto::PodSandboxFilter {
    fn from(f: t::PodSandboxFilter) -> Self {
        Self {
            id: f.id.unwrap_or_default(),
            state: f.state.map(|s| proto::PodSandboxStateValue {
                state: proto::PodSandboxState::from(s) as i32,
            }),
            ..Self::default()
        }
    }
}

impl From<t::ContainerFilter> for proto::ContainerFilter {
    fn from(f: t::ContainerFilter) -> Self {
        Self {
            id: f.id.unwrap_or_default(),
            pod_sandbox_id: f.pod_sandbox_id.unwrap_or_default(),
            state: f.state.map(|s| proto::ContainerStateValue {
                state: proto::ContainerState::from(s) as i32,
            }),
            ..Self::default()
        }
    }
}
