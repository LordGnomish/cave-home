// SPDX-License-Identifier: Apache-2.0
//! `PodLifecycleEvent` and `PodLifecycleEventType`.
//!
//! Hand-port of `pkg/kubelet/pleg/pleg.go`.

use crate::api::PodUid;

/// Type of lifecycle transition observed by the PLEG.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PodLifecycleEventType {
    /// `ContainerStarted` — a new container is now running.
    ContainerStarted,
    /// `ContainerDied` — a previously-running container exited.
    ContainerDied,
    /// `ContainerRemoved` — the runtime garbage-collected a container.
    ContainerRemoved,
    /// `PodSync` — full re-sync requested (sandbox change).
    PodSync,
    /// `ContainerChanged` — container changed without exiting (rare; placeholder).
    ContainerChanged,
}

/// One PLEG event — published by `GenericPleg` and consumed by the kubelet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PodLifecycleEvent {
    pub id: PodUid,
    /// Container ID — the CRI-side ID, empty for `PodSync`.
    pub container_id: String,
    pub event_type: PodLifecycleEventType,
}
