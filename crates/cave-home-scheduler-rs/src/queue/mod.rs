// SPDX-License-Identifier: Apache-2.0
//! Scheduling queue — drives the ordering of pods through `scheduleOne`.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/backend/queue/scheduling_queue.go

pub mod priority_queue;

pub use priority_queue::{PriorityQueue, QueuedPodInfo};

use crate::types::Pod;

/// Upstream: `pkg/scheduler/backend/queue/scheduling_queue.go::SchedulingQueue`.
pub trait SchedulingQueue: Send + Sync {
    /// Add a pod to the active queue. Upstream: `Add`.
    fn add(&self, pod: Pod);
    /// Pop the highest-priority schedulable pod (blocking semantics in
    /// upstream — here a non-blocking try-pop suits the `scheduleOne`
    /// driver). Returns `None` when the active queue is empty.
    fn pop(&self) -> Option<QueuedPodInfo>;
    /// Move a pod into the backoff queue after a failed attempt.
    /// Upstream: `AddUnschedulableIfNotPresent`.
    fn add_unschedulable(&self, pod: QueuedPodInfo);
    /// Promote due-back-off pods back to active.
    /// Upstream: `flushBackoffQCompleted`.
    fn flush_backoff(&self, now_ms: u64);
    /// Total number of pods in active + backoff queues.
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
