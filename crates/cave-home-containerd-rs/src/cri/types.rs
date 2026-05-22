// SPDX-License-Identifier: Apache-2.0
//! Rust ergonomic wrappers around the generated CRI proto types.
//!
//! These mirror `internal/cri/store/sandbox/Metadata` and
//! `internal/cri/store/container/Metadata` from upstream.

use std::time::SystemTime;

/// Sandbox state — matches `runtime.PodSandboxState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxState {
    /// Ready (running).
    Ready,
    /// NotReady (stopped).
    NotReady,
}

/// Container state — matches `runtime.ContainerState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    /// Created but not started.
    Created,
    /// Running.
    Running,
    /// Exited.
    Exited,
    /// Unknown.
    Unknown,
}

/// Sandbox metadata, immutable after creation.
///
/// Port of `internal/cri/store/sandbox/Metadata`. Every field that has
/// no Phase-1 lifecycle (e.g. `NetNS`) is captured as a string so we
/// don't lose it on the wire — we just don't act on it.
#[derive(Debug, Clone)]
pub struct SandboxMetadata {
    /// Stable sandbox ID (UUID v4).
    pub id: String,
    /// Pod name.
    pub name: String,
    /// Pod UID.
    pub uid: String,
    /// Pod namespace.
    pub namespace: String,
    /// Container runtime handler (e.g. "runc"). Recorded only.
    pub runtime_handler: String,
    /// Network namespace path (recorded only — Phase 1 does not call
    /// `unshare(2)`).
    pub net_ns_path: String,
    /// SELinux process label (recorded only).
    pub process_label: String,
    /// Creation timestamp.
    pub created_at: SystemTime,
}

/// Sandbox runtime status.
#[derive(Debug, Clone)]
pub struct SandboxStatus {
    /// Current state.
    pub state: SandboxState,
    /// Wall-clock state-change timestamp.
    pub state_changed_at: SystemTime,
}

/// Container metadata, immutable after creation.
#[derive(Debug, Clone)]
pub struct ContainerMetadata {
    /// Stable container ID.
    pub id: String,
    /// Owning sandbox ID.
    pub sandbox_id: String,
    /// Container name.
    pub name: String,
    /// Image reference (resolved by the CRI client).
    pub image: String,
    /// Container runtime handler.
    pub runtime_handler: String,
    /// Creation timestamp.
    pub created_at: SystemTime,
}

/// Container runtime status.
#[derive(Debug, Clone)]
pub struct ContainerStatus {
    /// Current state.
    pub state: ContainerState,
    /// Last started time (Unix epoch when never started).
    pub started_at: SystemTime,
    /// Last finished time.
    pub finished_at: SystemTime,
    /// Process exit code (0 when not exited).
    pub exit_code: i32,
    /// Reason for last state change.
    pub reason: String,
    /// Human-readable message.
    pub message: String,
}

impl Default for ContainerStatus {
    fn default() -> Self {
        Self {
            state: ContainerState::Created,
            started_at: SystemTime::UNIX_EPOCH,
            finished_at: SystemTime::UNIX_EPOCH,
            exit_code: 0,
            reason: String::new(),
            message: String::new(),
        }
    }
}
