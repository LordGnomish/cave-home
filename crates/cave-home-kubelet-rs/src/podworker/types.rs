// SPDX-License-Identifier: Apache-2.0
//! `WorkType`, `PodWorkerState`, `SyncOutcome`.
//!
//! Hand-port of the `WorkType` constants in `pkg/kubelet/pod_workers.go`.

/// Reason a pod-worker iteration was triggered.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WorkType {
    /// Normal "make actual match desired" pass.
    Sync,
    /// Pod has been removed from the API; tear it down (graceful).
    Terminating,
    /// Tear-down complete; perform final cleanup.
    Terminated,
}

/// Per-pod worker state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PodWorkerState {
    /// Not yet started.
    #[default]
    Idle,
    /// Currently processing a sync.
    Syncing,
    /// Waiting for the next sync trigger.
    Waiting,
    /// Pod is being torn down.
    Terminating,
    /// Pod is fully gone — worker can be reaped.
    Terminated,
}

/// Outcome of a single sync pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncOutcome {
    /// Sandbox CRI ID, after the sync.
    pub sandbox_id: Option<String>,
    /// Containers created during this sync (CRI IDs).
    pub created_containers: Vec<String>,
    /// Containers started during this sync.
    pub started_containers: Vec<String>,
    /// Containers killed during this sync.
    pub killed_containers: Vec<String>,
}

impl SyncOutcome {
    pub fn empty() -> Self {
        Self {
            sandbox_id: None,
            created_containers: Vec::new(),
            started_containers: Vec::new(),
            killed_containers: Vec::new(),
        }
    }
}
