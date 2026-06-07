// SPDX-License-Identifier: Apache-2.0
//! Priority-ordered scheduling queue with active and backoff sub-queues.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/backend/queue/scheduling_queue.go::PriorityQueue

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::SchedulingQueue;
use crate::types::Pod;

/// Upstream: `pkg/scheduler/backend/queue/scheduling_queue.go::QueuedPodInfo`.
#[derive(Debug, Clone)]
pub struct QueuedPodInfo {
    pub pod: Pod,
    pub attempts: u32,
    pub initial_attempt_ms: u64,
    pub last_attempt_ms: u64,
}

impl QueuedPodInfo {
    #[must_use]
    pub fn new(pod: Pod) -> Self {
        Self {
            pod,
            attempts: 0,
            initial_attempt_ms: 0,
            last_attempt_ms: 0,
        }
    }

    /// Upstream backoff is exponential with a `podInitialBackoffDuration`
    /// of 1s and a `podMaxBackoffDuration` of 10s. Same shape here.
    #[must_use]
    pub fn ready_at_ms(&self) -> u64 {
        const INITIAL_MS: u64 = 1_000;
        const MAX_MS: u64 = 10_000;
        if self.attempts == 0 {
            return 0;
        }
        let shift = self.attempts.saturating_sub(1).min(20);
        let raw = INITIAL_MS.saturating_mul(1_u64 << shift);
        let backoff = raw.min(MAX_MS);
        self.last_attempt_ms.saturating_add(backoff)
    }
}

#[derive(Debug)]
struct HeapEntry {
    priority: i32, // negated for max-heap semantics via BinaryHeap (which is max)
    seq: u64,
    info: QueuedPodInfo,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for HeapEntry {}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeapEntry {
    /// Upstream: higher Pod.Spec.Priority pops first; ties broken by
    /// earlier admission timestamp (here a monotonic seq).
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is max-heap. Sort by (priority desc, seq asc).
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

/// Upstream: `pkg/scheduler/backend/queue/scheduling_queue.go::PriorityQueue`.
#[derive(Default, Clone)]
pub struct PriorityQueue {
    inner: Arc<Mutex<PriorityQueueInner>>,
}

#[derive(Default)]
struct PriorityQueueInner {
    active: BinaryHeap<HeapEntry>,
    backoff: Vec<QueuedPodInfo>,
    seq: u64,
}

impl PriorityQueue {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl SchedulingQueue for PriorityQueue {
    fn add(&self, pod: Pod) {
        let info = QueuedPodInfo::new(pod);
        let mut g = self.inner.lock();
        g.seq += 1;
        let entry = HeapEntry {
            priority: info.pod.spec.priority,
            seq: g.seq,
            info,
        };
        g.active.push(entry);
    }

    fn pop(&self) -> Option<QueuedPodInfo> {
        let mut g = self.inner.lock();
        g.active.pop().map(|e| e.info)
    }

    fn add_unschedulable(&self, mut pod: QueuedPodInfo) {
        pod.attempts += 1;
        let mut g = self.inner.lock();
        g.backoff.push(pod);
    }

    fn flush_backoff(&self, now_ms: u64) {
        let mut g = self.inner.lock();
        let mut still = Vec::with_capacity(g.backoff.len());
        let due: Vec<QueuedPodInfo> =
            std::mem::take(&mut g.backoff)
                .into_iter()
                .filter_map(|info| {
                    if info.ready_at_ms() <= now_ms {
                        Some(info)
                    } else {
                        still.push(info.clone());
                        None
                    }
                })
                .collect();
        g.backoff = still;
        for info in due {
            g.seq += 1;
            let entry = HeapEntry {
                priority: info.pod.spec.priority,
                seq: g.seq,
                info,
            };
            g.active.push(entry);
        }
    }

    fn len(&self) -> usize {
        let g = self.inner.lock();
        g.active.len() + g.backoff.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ObjectMeta, Pod};

    fn pod(name: &str, priority: i32) -> Pod {
        let mut p = Pod::default();
        p.metadata = ObjectMeta {
            name: name.into(),
            uid: name.into(),
            ..Default::default()
        };
        p.spec.priority = priority;
        p
    }

    #[test]
    fn pop_returns_higher_priority_first() {
        let q = PriorityQueue::new();
        q.add(pod("low", 1));
        q.add(pod("high", 100));
        q.add(pod("mid", 50));

        assert_eq!(q.pop().unwrap().pod.metadata.name, "high");
        assert_eq!(q.pop().unwrap().pod.metadata.name, "mid");
        assert_eq!(q.pop().unwrap().pod.metadata.name, "low");
        assert!(q.pop().is_none());
    }

    #[test]
    fn ties_broken_by_admission_order() {
        let q = PriorityQueue::new();
        q.add(pod("first", 5));
        q.add(pod("second", 5));
        assert_eq!(q.pop().unwrap().pod.metadata.name, "first");
        assert_eq!(q.pop().unwrap().pod.metadata.name, "second");
    }

    #[test]
    fn len_counts_active_and_backoff() {
        let q = PriorityQueue::new();
        q.add(pod("a", 0));
        q.add(pod("b", 0));
        assert_eq!(q.len(), 2);

        let info = q.pop().unwrap();
        q.add_unschedulable(info);
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn flush_backoff_promotes_due_pods() {
        let q = PriorityQueue::new();
        let mut info = QueuedPodInfo::new(pod("retry", 0));
        info.last_attempt_ms = 0;
        info.attempts = 0; // ready_at == 0
        q.add_unschedulable(info);
        // After add_unschedulable attempts = 1, ready_at = 0 + 1000.
        q.flush_backoff(0);
        assert!(q.pop().is_none()); // not yet
        q.flush_backoff(2_000);
        assert_eq!(q.pop().unwrap().pod.metadata.name, "retry");
    }

    use crate::framework::{ActionType, ClusterEvent, Gvk};

    #[test]
    fn add_unschedulable_if_not_present_lands_in_unschedulable_set() {
        let q = PriorityQueue::new();
        // Pod failed this scheduling cycle; no concurrent move happened.
        let cycle = q.scheduling_cycle();
        let info = QueuedPodInfo::new(pod("p", 0));
        q.add_unschedulable_if_not_present(info, cycle, 0);
        // It is NOT in the active queue.
        assert!(q.pop().is_none());
        assert_eq!(q.unschedulable_count(), 1);
    }

    #[test]
    fn move_all_reactivates_matching_unschedulable_pod() {
        let q = PriorityQueue::new();
        let cycle = q.scheduling_cycle();
        q.add_unschedulable_if_not_present(QueuedPodInfo::new(pod("p", 0)), cycle, 0);
        // A node was added — reconsider waiting pods.
        let ev = ClusterEvent::new(Gvk::Node, ActionType::ADD);
        q.move_all_to_active_or_backoff_queue(&ev, 0);
        assert_eq!(q.unschedulable_count(), 0);
        assert_eq!(q.pop().unwrap().pod.metadata.name, "p");
    }

    #[test]
    fn add_after_concurrent_move_routes_to_backoff_not_unschedulable() {
        let q = PriorityQueue::new();
        q.add(pod("p", 0));
        // Pop captures the scheduling cycle this pod is being scheduled in.
        let popped = q.pop().unwrap();
        let pod_cycle = q.scheduling_cycle();
        // A cluster event arrives WHILE the pod is mid-flight.
        let ev = ClusterEvent::new(Gvk::Node, ActionType::ADD);
        q.move_all_to_active_or_backoff_queue(&ev, 0);
        // The failed pod must not be parked in unschedulable (it would miss
        // the move it raced); it goes to backoff so it is retried.
        q.add_unschedulable_if_not_present(popped, pod_cycle, 0);
        assert_eq!(q.unschedulable_count(), 0);
        assert_eq!(q.backoff_count(), 1);
    }

    #[test]
    fn flush_unschedulable_leftover_moves_stale_pod() {
        let q = PriorityQueue::new();
        let cycle = q.scheduling_cycle();
        q.add_unschedulable_if_not_present(QueuedPodInfo::new(pod("old", 0)), cycle, 0);
        // Not yet stale.
        q.flush_unschedulable_pods_leftover(30_000);
        assert_eq!(q.unschedulable_count(), 1);
        // Past the 60s leftover threshold — must be moved out.
        q.flush_unschedulable_pods_leftover(61_000);
        assert_eq!(q.unschedulable_count(), 0);
    }

    #[test]
    fn move_all_to_backoff_when_pod_is_backing_off() {
        let q = PriorityQueue::new();
        let cycle = q.scheduling_cycle();
        let mut info = QueuedPodInfo::new(pod("p", 0));
        info.attempts = 1; // backoff = 1s
        info.last_attempt_ms = 0;
        q.add_unschedulable_if_not_present(info, cycle, 0);
        let ev = ClusterEvent::new(Gvk::Node, ActionType::ADD);
        // Move while the pod is still inside its backoff window.
        q.move_all_to_active_or_backoff_queue(&ev, 500);
        // It must not be immediately poppable — it is in backoff.
        assert!(q.pop().is_none());
        assert_eq!(q.backoff_count(), 1);
        // After its backoff elapses it surfaces to active.
        q.flush_backoff(2_000);
        assert_eq!(q.pop().unwrap().pod.metadata.name, "p");
    }

    #[test]
    fn backoff_grows_exponentially_then_caps() {
        let info = QueuedPodInfo {
            attempts: 1,
            initial_attempt_ms: 0,
            last_attempt_ms: 0,
            ..QueuedPodInfo::new(pod("p", 0))
        };
        assert_eq!(info.ready_at_ms(), 1_000); // 1s

        let info2 = QueuedPodInfo {
            attempts: 4,
            ..info.clone()
        };
        // 1s * 2^3 = 8s
        assert_eq!(info2.ready_at_ms(), 8_000);

        let info3 = QueuedPodInfo {
            attempts: 50,
            ..info
        };
        assert_eq!(info3.ready_at_ms(), 10_000); // capped at 10s
    }
}
