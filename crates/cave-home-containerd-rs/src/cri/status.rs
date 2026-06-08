// SPDX-License-Identifier: Apache-2.0
//! CRI `PodSandbox` + `Container` status computation.
//!
//! Behavioural reimplementation of the documented CRI v1 status model: a
//! `PodSandbox` is `READY` while its sandbox process is alive and `NOTREADY`
//! once torn down; a `Container` maps the runtime task state onto the CRI
//! `ContainerState` enum (`CREATED` / `RUNNING` / `EXITED` / `UNKNOWN`).
//!
//! Spec sources:
//!   * CRI `runtime/v1/api.proto` `PodSandboxState`
//!     (`SANDBOX_READY` / `SANDBOX_NOTREADY`), `PodSandboxStatus`.
//!   * CRI `ContainerState` (`CONTAINER_CREATED` / `CONTAINER_RUNNING` /
//!     `CONTAINER_EXITED` / `CONTAINER_UNKNOWN`), `ContainerStatus`.

use crate::lifecycle::TaskState;

/// CRI pod-sandbox readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxState {
    /// `SANDBOX_READY` — the sandbox infra is up; containers may run in it.
    Ready,
    /// `SANDBOX_NOTREADY` — the sandbox has been stopped/torn down.
    NotReady,
}

impl SandboxState {
    /// The CRI enum token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "SANDBOX_READY",
            Self::NotReady => "SANDBOX_NOTREADY",
        }
    }
}

/// CRI container state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    /// `CONTAINER_CREATED`.
    Created,
    /// `CONTAINER_RUNNING`.
    Running,
    /// `CONTAINER_EXITED`.
    Exited,
    /// `CONTAINER_UNKNOWN` — runtime state could not be determined.
    Unknown,
}

impl ContainerState {
    /// The CRI enum token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "CONTAINER_CREATED",
            Self::Running => "CONTAINER_RUNNING",
            Self::Exited => "CONTAINER_EXITED",
            Self::Unknown => "CONTAINER_UNKNOWN",
        }
    }
}

/// A pod sandbox: the shared network/IPC environment containers live in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodSandbox {
    /// The sandbox id.
    pub id: String,
    /// The sandbox name (kube `metadata.name`).
    pub name: String,
    /// Whether the sandbox infra process is alive.
    pub infra_alive: bool,
}

/// A container scheduled into a sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    /// The container id.
    pub id: String,
    /// The owning sandbox id.
    pub sandbox_id: String,
    /// The runtime task state.
    pub task_state: TaskState,
    /// The image reference the container runs.
    pub image: String,
}

/// Computes the CRI sandbox status from the sandbox's infra liveness.
///
/// A sandbox is `READY` only while its infra process is alive; otherwise it is
/// `NOTREADY` (the documented `RunPodSandbox` / `StopPodSandbox` contract).
#[must_use]
pub const fn sandbox_status(sandbox: &PodSandbox) -> SandboxState {
    if sandbox.infra_alive {
        SandboxState::Ready
    } else {
        SandboxState::NotReady
    }
}

/// Maps a runtime task state onto the CRI `ContainerState`.
///
/// `created -> CREATED`, `running`/`paused -> RUNNING` (CRI has no paused
/// state; a frozen container is still reported running), `stopped -> EXITED`.
/// A container whose owning sandbox is not ready is reported `UNKNOWN` unless
/// it has already exited (a terminal exit is always reportable).
#[must_use]
pub fn container_status(container: &Container, sandbox: &PodSandbox) -> ContainerState {
    let base = match container.task_state {
        TaskState::Created => ContainerState::Created,
        TaskState::Running | TaskState::Paused => ContainerState::Running,
        TaskState::Stopped => ContainerState::Exited,
    };
    // If the sandbox infra is gone, a non-terminal container's true state is
    // unknowable from the runtime; report UNKNOWN. A stopped container keeps
    // its terminal EXITED state.
    if !sandbox.infra_alive && base != ContainerState::Exited {
        ContainerState::Unknown
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_sandbox() -> PodSandbox {
        PodSandbox { id: "s1".to_owned(), name: "pod".to_owned(), infra_alive: true }
    }

    fn container(state: TaskState, sandbox_id: &str) -> Container {
        Container {
            id: "c1".to_owned(),
            sandbox_id: sandbox_id.to_owned(),
            task_state: state,
            image: "docker.io/library/nginx:latest".to_owned(),
        }
    }

    #[test]
    fn live_sandbox_is_ready() {
        assert_eq!(sandbox_status(&ready_sandbox()), SandboxState::Ready);
    }

    #[test]
    fn dead_sandbox_is_notready() {
        let mut s = ready_sandbox();
        s.infra_alive = false;
        assert_eq!(sandbox_status(&s), SandboxState::NotReady);
    }

    #[test]
    fn created_task_maps_to_created() {
        let s = ready_sandbox();
        let c = container(TaskState::Created, "s1");
        assert_eq!(container_status(&c, &s), ContainerState::Created);
    }

    #[test]
    fn running_and_paused_map_to_running() {
        let s = ready_sandbox();
        assert_eq!(
            container_status(&container(TaskState::Running, "s1"), &s),
            ContainerState::Running
        );
        assert_eq!(
            container_status(&container(TaskState::Paused, "s1"), &s),
            ContainerState::Running
        );
    }

    #[test]
    fn stopped_task_maps_to_exited() {
        let s = ready_sandbox();
        assert_eq!(
            container_status(&container(TaskState::Stopped, "s1"), &s),
            ContainerState::Exited
        );
    }

    #[test]
    fn running_container_in_dead_sandbox_is_unknown() {
        let mut s = ready_sandbox();
        s.infra_alive = false;
        assert_eq!(
            container_status(&container(TaskState::Running, "s1"), &s),
            ContainerState::Unknown
        );
    }

    #[test]
    fn exited_container_stays_exited_even_in_dead_sandbox() {
        let mut s = ready_sandbox();
        s.infra_alive = false;
        assert_eq!(
            container_status(&container(TaskState::Stopped, "s1"), &s),
            ContainerState::Exited
        );
    }
}
