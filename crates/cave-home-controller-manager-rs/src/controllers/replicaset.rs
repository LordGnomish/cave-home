// SPDX-License-Identifier: Apache-2.0
//! `ReplicaSet` controller — the replica-count reconcile decision.
//!
//! Behavioural reimplementation of the documented `ReplicaSet` controller
//! contract (`pkg/controller/replicaset/replica_set.go::manageReplicas`): given
//! a `ReplicaSet`'s desired replica count + label selector and the current pods,
//! decide whether to **create** the missing replicas or **delete** the excess,
//! and — when deleting — which pods to evict first. The victim ordering mirrors
//! the shared helper `pkg/controller/controller_utils.go`
//! (`ActivePodsWithRanks.Less` / `getPodsToDelete`).
//!
//! This is the pure decision only: actually creating/deleting pods against the
//! apiserver, the slow-start batching of `manageReplicas`, expectations
//! tracking, and pod adoption/orphaning via `controllerRef` are the deferred
//! client phase (see `parity.manifest.toml`). `std` only, panic-free.
//!
//! ## Active pods
//!
//! Counting follows `controller.FilterActivePods`: a pod is *active* when its
//! phase is neither `Succeeded` nor `Failed` **and** it has no deletion
//! timestamp. Only pods whose labels match the `ReplicaSet` selector are
//! considered owned for this decision (adoption/release is deferred).
//!
//! ## Victim ordering
//!
//! When scaling down, pods are ranked so the *least valuable* are deleted
//! first, comparing in this priority order (upstream `ActivePodsWithRanks`):
//!
//! 1. **Unassigned** (no node) before assigned.
//! 2. **Less-ready phase**: `Pending` (0) < `Unknown` (1) < `Running` (2).
//! 3. **Not-ready** before ready.
//! 4. **Lower** `pod-deletion-cost` before higher.
//! 5. **Higher** restart count before lower.
//! 6. **Younger** (later creation timestamp) before older.
//!
//! The pod-colocation rank (upstream tie-break by how many same-controller pods
//! share a node) is deferred; the `uid` is used as a final deterministic
//! tie-break instead.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use crate::types::Uid;

/// Pod lifecycle phase (apimachinery `PodPhase` subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodPhase {
    /// Accepted but not yet running on a node.
    Pending,
    /// Bound to a node with at least one container running.
    Running,
    /// All containers terminated successfully; will not restart.
    Succeeded,
    /// All containers terminated and at least one failed.
    Failed,
    /// State could not be obtained (typically a lost node).
    Unknown,
}

impl PodPhase {
    /// Ordinal used by the victim sort: lower phases are deleted first
    /// (upstream `podPhaseToOrdinal`: Pending < Unknown < Running). The
    /// terminal phases never reach the sort (they are filtered as inactive)
    /// but are ordered after `Running` for totality.
    const fn delete_ordinal(self) -> u8 {
        match self {
            Self::Pending => 0,
            Self::Unknown => 1,
            Self::Running => 2,
            Self::Succeeded | Self::Failed => 3,
        }
    }

    /// `true` for the terminal phases that make a pod inactive.
    const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed)
    }
}

/// The pod fields the `ReplicaSet` decision reads.
///
/// A behavioural subset of `v1.Pod` — labels for selector matching, the
/// scheduling/health signals the victim ranking consults, and the deletion
/// timestamp that marks a pod inactive.
#[derive(Debug, Clone)]
pub struct PodView {
    /// Stable identity; what a delete decision returns.
    pub uid: Uid,
    /// Labels matched against the `ReplicaSet` selector.
    pub labels: BTreeMap<String, String>,
    /// Assigned node name; empty means unscheduled.
    pub node_name: String,
    /// Lifecycle phase.
    pub phase: PodPhase,
    /// Whether the pod's `Ready` condition is true.
    pub ready: bool,
    /// Aggregate container restart count.
    pub restart_count: i32,
    /// The `controller.kubernetes.io/pod-deletion-cost` hint; lower-cost pods
    /// are deleted first. Defaults to 0.
    pub deletion_cost: i64,
    /// Creation time in caller-supplied epoch seconds; younger pods (larger
    /// value) are deleted first.
    pub creation_timestamp: i64,
    /// Deletion time, if the pod is already terminating. `Some` ⇒ inactive.
    pub deletion_timestamp: Option<i64>,
}

impl PodView {
    /// A running, ready, assigned pod with the given uid — the *least*
    /// preferred deletion victim. Builders below relax it for tests/callers.
    #[must_use]
    pub fn running(uid: &str) -> Self {
        Self {
            uid: uid.to_owned(),
            labels: BTreeMap::new(),
            node_name: "node".to_owned(),
            phase: PodPhase::Running,
            ready: true,
            restart_count: 0,
            deletion_cost: 0,
            creation_timestamp: 0,
            deletion_timestamp: None,
        }
    }

    /// Add a label, builder-style.
    #[must_use]
    pub fn with_label(mut self, key: &str, value: &str) -> Self {
        self.labels.insert(key.to_owned(), value.to_owned());
        self
    }

    /// Pin the pod to a node.
    #[must_use]
    pub fn on_node(mut self, node: &str) -> Self {
        node.clone_into(&mut self.node_name);
        self
    }

    /// Mark the pod as unscheduled (no node).
    #[must_use]
    pub fn unassigned(mut self) -> Self {
        self.node_name.clear();
        self
    }

    /// Set the lifecycle phase.
    #[must_use]
    pub const fn with_phase(mut self, phase: PodPhase) -> Self {
        self.phase = phase;
        self
    }

    /// Mark the pod as not ready.
    #[must_use]
    pub const fn not_ready(mut self) -> Self {
        self.ready = false;
        self
    }

    /// Set the aggregate restart count.
    #[must_use]
    pub const fn with_restarts(mut self, restarts: i32) -> Self {
        self.restart_count = restarts;
        self
    }

    /// Set the pod-deletion-cost hint.
    #[must_use]
    pub const fn with_deletion_cost(mut self, cost: i64) -> Self {
        self.deletion_cost = cost;
        self
    }

    /// Set the creation timestamp.
    #[must_use]
    pub const fn created_at(mut self, ts: i64) -> Self {
        self.creation_timestamp = ts;
        self
    }

    /// Mark the pod terminating at the given timestamp.
    #[must_use]
    pub const fn terminating_at(mut self, ts: i64) -> Self {
        self.deletion_timestamp = Some(ts);
        self
    }

    /// `controller.FilterActivePods`: not terminal and not terminating.
    #[must_use]
    const fn is_active(&self) -> bool {
        !self.phase.is_terminal() && self.deletion_timestamp.is_none()
    }

    /// The ascending sort key; the pod that sorts **first** is deleted first.
    fn delete_rank(&self) -> (u8, u8, u8, i64, Reverse<i32>, Reverse<i64>, &str) {
        (
            u8::from(!self.node_name.is_empty()), // unassigned (0) first
            self.phase.delete_ordinal(),          // less-ready phase first
            u8::from(self.ready),                 // not-ready (0) first
            self.deletion_cost,                   // lower cost first
            Reverse(self.restart_count),          // higher restarts first
            Reverse(self.creation_timestamp),     // younger first
            self.uid.as_str(),                    // deterministic tie-break
        )
    }
}

/// The replica-management inputs: desired count + selector.
#[derive(Debug, Clone)]
pub struct ReplicaSetSpec {
    /// Desired replica count. Negative values are clamped to zero.
    pub replicas: i32,
    /// `matchLabels` selector: every entry must be present and equal on a pod.
    pub selector: BTreeMap<String, String>,
}

impl ReplicaSetSpec {
    /// A spec with the given replica count and an empty selector.
    #[must_use]
    pub const fn new(replicas: i32) -> Self {
        Self {
            replicas,
            selector: BTreeMap::new(),
        }
    }

    /// Add a `matchLabels` entry, builder-style.
    #[must_use]
    pub fn select(mut self, key: &str, value: &str) -> Self {
        self.selector.insert(key.to_owned(), value.to_owned());
        self
    }

    /// Desired count, clamped to a non-negative `usize`.
    #[must_use]
    fn desired(&self) -> usize {
        usize::try_from(self.replicas.max(0)).unwrap_or(0)
    }

    /// `matchLabels` semantics: every selector entry present and equal.
    /// An empty selector matches every pod (the apimachinery `Everything`
    /// case); a valid `ReplicaSet` always carries a non-empty selector.
    #[must_use]
    fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        self.selector
            .iter()
            .all(|(k, v)| labels.get(k).is_some_and(|got| got == v))
    }
}

/// The reconcile decision for one `ReplicaSet`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplicaSetAction {
    /// Create this many new pods to reach the desired count.
    CreatePods(usize),
    /// Delete these pods (by uid), ordered most-disposable first.
    DeletePods(Vec<Uid>),
    /// Active replicas already match the desired count.
    InSync,
}

/// Decide what to do to drive `pods` toward `spec.replicas`.
///
/// Counts active selector-matching pods, then returns the create count, the
/// ordered delete set, or [`ReplicaSetAction::InSync`].
#[must_use]
pub fn reconcile(spec: &ReplicaSetSpec, pods: &[PodView]) -> ReplicaSetAction {
    let mut active: Vec<&PodView> = pods
        .iter()
        .filter(|p| p.is_active() && spec.matches(&p.labels))
        .collect();

    let desired = spec.desired();
    let current = active.len();

    match current.cmp(&desired) {
        std::cmp::Ordering::Less => ReplicaSetAction::CreatePods(desired - current),
        std::cmp::Ordering::Greater => {
            active.sort_by(|a, b| a.delete_rank().cmp(&b.delete_rank()));
            let victims = active
                .iter()
                .take(current - desired)
                .map(|p| p.uid.clone())
                .collect();
            ReplicaSetAction::DeletePods(victims)
        }
        std::cmp::Ordering::Equal => ReplicaSetAction::InSync,
    }
}
