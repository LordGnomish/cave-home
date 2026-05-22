// SPDX-License-Identifier: Apache-2.0
//! Per-pod state machine that reconciles desired -> actual via the CRI.
//!
//! Hand-port of `pkg/kubelet/pod_workers.go` (v1.36.1).

pub mod types;
pub mod worker;

pub use types::{PodWorkerState, SyncOutcome, WorkType};
pub use worker::{PodWorker, PodWorkerError};
