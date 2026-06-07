// SPDX-License-Identifier: Apache-2.0
//! Priority-ordered scheduling queue with active and backoff sub-queues.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         `pkg/scheduler/backend/queue/scheduling_queue.go::PriorityQueue`

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

use parking_lot::Mutex;

use super::SchedulingQueue;
use crate::framework::{ClusterEvent, WILD_CARD_EVENT};
use crate::types::Pod;

/// Upstream: `pkg/scheduler/backend/queue/scheduling_queue.go::QueuedPodInfo`.
#[derive(Debug, Clone)]
pub struct QueuedPodInfo {
    pub pod: Pod,
    pub attempts: u32,
    pub initial_attempt_ms: u64,
    pub last_attempt_ms: u64,
    /// When the pod most recently entered the unschedulable set (ms epoch).
    /// Upstream: `QueuedPodInfo.Timestamp`, consulted by
    /// `flushUnschedulablePodsLeftover`.
    pub unschedulable_since_ms: u64,
    /// Cluster events that, if observed, should re-activate this pod.
    /// Upstream builds this per-pod from the union of the plugins'
    /// `EventsToRegister`. Phase 2 defaults to [`WILD_CARD_EVENT`] (any cluster
    /// change reconsiders the pod) because hint-driven re-queue (`QueueingHints`)
    /// is deferred — see `parity.manifest.toml`.
    pub registered_events: Vec<ClusterEvent>,
}

impl QueuedPodInfo {
    #[must_use]
    pub fn new(pod: Pod) -> Self {
        Self {
            pod,
            attempts: 0,
            initial_attempt_ms: 0,
            last_attempt_ms: 0,
            unschedulable_since_ms: 0,
            registered_events: vec![WILD_CARD_EVENT],
        }
    }

    /// Upstream: `isPodBackingoff` — true while the pod is still inside its
    /// exponential-backoff window at `now_ms`.
    #[must_use]
    pub fn is_backing_off(&self, now_ms: u64) -> bool {
        self.ready_at_ms() > now_ms
    }

    /// Upstream: `podMatchesEvent` — does any of this pod's registered events
    /// match the event that just occurred?
    #[must_use]
    pub fn matches_event(&self, occurred: &ClusterEvent) -> bool {
        self.registered_events.iter().any(|e| e.matches(occurred))
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

struct PriorityQueueInner {
    active: BinaryHeap<HeapEntry>,
    backoff: Vec<QueuedPodInfo>,
    /// Upstream: `unschedulablePods` — pods that failed scheduling and are
    /// parked until a cluster event (or the leftover timeout) reconsiders
    /// them. Keyed by pod uid.
    unschedulable: HashMap<String, QueuedPodInfo>,
    seq: u64,
    /// Upstream: `schedulingCycle` — incremented on every `pop`.
    scheduling_cycle: u64,
    /// Upstream: `moveRequestCycle` — the `schedulingCycle` at which the last
    /// `MoveAllToActiveOrBackoffQueue` ran. `None` means "never" (upstream's
    /// `-1` sentinel).
    move_request_cycle: Option<u64>,
}

impl Default for PriorityQueueInner {
    fn default() -> Self {
        Self {
            active: BinaryHeap::new(),
            backoff: Vec::new(),
            unschedulable: HashMap::new(),
            seq: 0,
            scheduling_cycle: 0,
            move_request_cycle: None,
        }
    }
}

/// Upstream: `pkg/scheduler/backend/queue/scheduling_queue.go::
/// unschedulablePodsLeftoverDuration` — pods sitting unschedulable longer than
/// this are flushed back so a missed event can never strand them forever.
const UNSCHEDULABLE_LEFTOVER_MS: u64 = 60_000;

impl PriorityQueue {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upstream: `PriorityQueue.SchedulingCycle`.
    #[must_use]
    pub fn scheduling_cycle(&self) -> u64 {
        self.inner.lock().scheduling_cycle
    }

    /// Number of pods currently parked in the unschedulable set.
    #[must_use]
    pub fn unschedulable_count(&self) -> usize {
        self.inner.lock().unschedulable.len()
    }

    /// Number of pods currently in the backoff sub-queue.
    #[must_use]
    pub fn backoff_count(&self) -> usize {
        self.inner.lock().backoff.len()
    }

    /// Push a queued pod onto the active heap (caller holds the lock).
    fn push_active(g: &mut PriorityQueueInner, info: QueuedPodInfo) {
        g.seq += 1;
        let entry = HeapEntry {
            priority: info.pod.spec.priority,
            seq: g.seq,
            info,
        };
        g.active.push(entry);
    }

    /// Upstream: `PriorityQueue.AddUnschedulableIfNotPresent`.
    ///
    /// A pod that failed its scheduling attempt is parked. If a
    /// `MoveAllToActiveOrBackoffQueue` ran *during* this pod's scheduling cycle
    /// (`move_request_cycle >= pod_scheduling_cycle`), or the pod is still
    /// backing off, it is sent to the backoff queue so it is retried promptly
    /// — otherwise it would miss the move it raced and sit idle. Otherwise it
    /// is parked in the unschedulable set to await a relevant cluster event.
    pub fn add_unschedulable_if_not_present(
        &self,
        mut info: QueuedPodInfo,
        pod_scheduling_cycle: u64,
        now_ms: u64,
    ) {
        info.attempts += 1;
        info.last_attempt_ms = now_ms;
        let key = info.pod.metadata.uid.clone();
        let mut g = self.inner.lock();
        // `IfNotPresent`: do not duplicate a pod already tracked.
        if g.unschedulable.contains_key(&key) {
            return;
        }
        // Routing is decided solely by the move-cycle race (upstream): if a
        // move happened during this pod's scheduling cycle it goes to backoff
        // so it is retried, else it parks in unschedulable. Whether a *moved*
        // pod then lands in active vs backoff is the `is_backing_off` decision,
        // applied in `move_all_to_active_or_backoff_queue`.
        if matches!(g.move_request_cycle, Some(m) if m >= pod_scheduling_cycle) {
            g.backoff.push(info);
        } else {
            info.unschedulable_since_ms = now_ms;
            g.unschedulable.insert(key, info);
        }
    }

    /// Upstream: `PriorityQueue.MoveAllToActiveOrBackoffQueue`.
    ///
    /// A cluster mutation occurred; every unschedulable pod whose registered
    /// events match is moved out — to active if its backoff has elapsed, else
    /// to backoff. Records the move against the current scheduling cycle so a
    /// pod that raced this move is not parked back into the unschedulable set.
    pub fn move_all_to_active_or_backoff_queue(&self, event: &ClusterEvent, now_ms: u64) {
        let mut g = self.inner.lock();
        let matched: Vec<String> = g
            .unschedulable
            .iter()
            .filter(|(_, info)| info.matches_event(event))
            .map(|(k, _)| k.clone())
            .collect();
        for key in matched {
            if let Some(info) = g.unschedulable.remove(&key) {
                if info.is_backing_off(now_ms) {
                    g.backoff.push(info);
                } else {
                    Self::push_active(&mut g, info);
                }
            }
        }
        g.move_request_cycle = Some(g.scheduling_cycle);
    }

    /// Upstream: `PriorityQueue.flushUnschedulablePodsLeftover`.
    ///
    /// Safety net: pods that have been unschedulable longer than
    /// [`UNSCHEDULABLE_LEFTOVER_MS`] are moved out regardless of events, so a
    /// dropped/missed cluster event can never strand a pod permanently.
    pub fn flush_unschedulable_pods_leftover(&self, now_ms: u64) {
        let mut g = self.inner.lock();
        let stale: Vec<String> = g
            .unschedulable
            .iter()
            .filter(|(_, info)| {
                now_ms.saturating_sub(info.unschedulable_since_ms) >= UNSCHEDULABLE_LEFTOVER_MS
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in stale {
            if let Some(info) = g.unschedulable.remove(&key) {
                if info.is_backing_off(now_ms) {
                    g.backoff.push(info);
                } else {
                    Self::push_active(&mut g, info);
                }
            }
        }
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
        // Upstream increments `schedulingCycle` on every Pop so a concurrent
        // move can be ordered against the cycle a pod is being scheduled in.
        g.scheduling_cycle += 1;
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
        g.active.len() + g.backoff.len() + g.unschedulable.len()
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
        // A node was added after the pod's 1s backoff elapsed — reconsider it.
        let ev = ClusterEvent::new(Gvk::Node, ActionType::ADD);
        q.move_all_to_active_or_backoff_queue(&ev, 2_000);
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
        // First failed attempt at t=0 → attempts=1, backoff window = 1s.
        q.add_unschedulable_if_not_present(QueuedPodInfo::new(pod("p", 0)), cycle, 0);
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
