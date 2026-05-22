// SPDX-License-Identifier: Apache-2.0
//! Pod Lifecycle Event Generator.
//!
//! Hand-port of `pkg/kubelet/pleg/` (`generic.go`, `pleg.go`).
//!
//! The PLEG periodically calls `CriClient::list_pod_sandbox` +
//! `list_containers`, diffs the result against the previous snapshot, and
//! broadcasts `PodLifecycleEvent`s on a channel that the kubelet drives the
//! `PodWorker`s with.
//!
//! Phase 1 ports:
//! - `pleg::clock` — `Clock` trait + `SystemClock` + `MockClock`.
//! - `pleg::types` — `PodLifecycleEvent`, `PodLifecycleEventType`.
//! - `pleg::pod_record` — per-pod cached "last seen" snapshot.
//! - `pleg::generic` — `GenericPleg::relist` + diff.

pub mod clock;
pub mod generic;
pub mod pod_record;
pub mod types;

pub use clock::{Clock, MockClock, SystemClock};
pub use generic::GenericPleg;
pub use pod_record::PodRecord;
pub use types::{PodLifecycleEvent, PodLifecycleEventType};
