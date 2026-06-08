// SPDX-License-Identifier: Apache-2.0
//! Scheduling framework — plugin trait surface.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/interface.go
//!
//! All nine framework extension points are present: `PreFilterPlugin`,
//! `FilterPlugin`, `PostFilterPlugin`, `PreScorePlugin`, `ScorePlugin`
//! (with the optional `NormalizeScore` rescale), `ReservePlugin`,
//! `PermitPlugin`, `PreBindPlugin`, `BindPlugin`, and `PostBindPlugin`.
//! The timed `Permit` "wait" disposition and `QueueingHints` remain
//! deferred — see `parity.manifest.toml`.

pub mod cycle_state;
pub mod events;
pub mod registry;

pub use cycle_state::CycleState;
pub use events::{ActionType, ClusterEvent, Gvk, WILD_CARD_EVENT};
// `NodeScore` is the per-node score pair handed to `ScorePlugin::normalize_score`.
// `events` provides the cluster-event vocabulary (`ActionType`/`Gvk`/`ClusterEvent`).
pub use registry::{PluginRegistry, RegistryBuilder};

use crate::cache::NodeInfo;
use crate::types::Pod;

/// Upstream: `pkg/scheduler/framework/types.go::Code`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Code {
    Success,
    Unschedulable,
    UnschedulableAndUnresolvable,
    /// A [`PermitPlugin`] asks the framework to hold the pod in the binding
    /// cycle until it is explicitly approved (or its timeout elapses).
    /// Upstream: `framework.Wait`.
    Wait,
    Error,
    Skip,
}

/// Upstream: `pkg/scheduler/framework/types.go::Status`.
#[derive(Debug, Clone)]
pub struct Status {
    pub code: Code,
    pub plugin: String,
    pub reasons: Vec<String>,
}

impl Status {
    #[must_use]
    pub fn success() -> Self {
        Self {
            code: Code::Success,
            plugin: String::new(),
            reasons: Vec::new(),
        }
    }

    /// Upstream: `framework.NewStatus(framework.Unschedulable, reasons...)`.
    #[must_use]
    pub fn unschedulable(plugin: &str, reason: impl Into<String>) -> Self {
        Self {
            code: Code::Unschedulable,
            plugin: plugin.into(),
            reasons: vec![reason.into()],
        }
    }

    /// Upstream: `framework.NewStatus(framework.UnschedulableAndUnresolvable, ...)`.
    #[must_use]
    pub fn unresolvable(plugin: &str, reason: impl Into<String>) -> Self {
        Self {
            code: Code::UnschedulableAndUnresolvable,
            plugin: plugin.into(),
            reasons: vec![reason.into()],
        }
    }

    /// Upstream: `framework.NewStatus(framework.Skip)` — a plugin abstains.
    /// Used by [`BindPlugin`] to decline a pod so the next bind plugin (or the
    /// `DefaultBinder`) handles it.
    #[must_use]
    pub fn skip(plugin: &str) -> Self {
        Self {
            code: Code::Skip,
            plugin: plugin.into(),
            reasons: Vec::new(),
        }
    }

    /// Upstream: `framework.NewStatus(framework.Wait)` — a [`PermitPlugin`]
    /// holds the pod until [`WaitingPod::allow`] / [`WaitingPod::reject`] is
    /// called (or the plugin's timeout elapses, which the framework treats as a
    /// rejection). See [`PermitPlugin::permit`].
    #[must_use]
    pub fn wait(plugin: &str) -> Self {
        Self {
            code: Code::Wait,
            plugin: plugin.into(),
            reasons: Vec::new(),
        }
    }

    /// Upstream: `framework.AsStatus(err)` — error-class status.
    #[must_use]
    pub fn error(plugin: &str, reason: impl Into<String>) -> Self {
        Self {
            code: Code::Error,
            plugin: plugin.into(),
            reasons: vec![reason.into()],
        }
    }

    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self.code, Code::Success | Code::Skip)
    }

    /// True for the [`Code::Wait`] disposition returned by a [`PermitPlugin`]
    /// that wants to hold the pod. Upstream: `Status.IsWait`.
    #[must_use]
    pub fn is_wait(&self) -> bool {
        self.code == Code::Wait
    }

    #[must_use]
    pub fn message(&self) -> String {
        self.reasons.join("; ")
    }
}

/// Upstream: `pkg/scheduler/framework/types.go::MinNodeScore / MaxNodeScore`.
pub const MIN_NODE_SCORE: i64 = 0;
pub const MAX_NODE_SCORE: i64 = 100;

/// Upstream: `pkg/scheduler/framework/types.go::NodeScore`.
///
/// A single (node, raw-score) pair produced by a [`ScorePlugin`]. The whole
/// slice for one plugin is handed to [`ScorePlugin::normalize_score`] so the
/// plugin can rescale the set into the `[MIN_NODE_SCORE, MAX_NODE_SCORE]` range
/// before weights are applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeScore {
    pub name: String,
    pub score: i64,
}

impl NodeScore {
    #[must_use]
    pub fn new(name: impl Into<String>, score: i64) -> Self {
        Self {
            name: name.into(),
            score,
        }
    }
}

/// Upstream: `pkg/scheduler/framework/interface.go::PreFilterResult`.
///
/// An optional restriction of the candidate node set produced by a PreFilter
/// plugin. `node_names == None` means "no opinion — keep all nodes"; multiple
/// PreFilter results are intersected.
#[derive(Debug, Clone, Default)]
pub struct PreFilterResult {
    pub node_names: Option<std::collections::BTreeSet<String>>,
}

/// Upstream: `pkg/scheduler/framework/interface.go::PreEnqueuePlugin`.
///
/// Runs before a pod is admitted to the active scheduling queue. Every
/// registered PreEnqueue plugin must return [`Code::Success`] for the pod to
/// become schedulable; if any returns a non-success status the pod is held out
/// of the active queue (parked, gated) until a cluster event re-evaluates it.
/// This is purely a gate — it never narrows nodes and runs entirely off the
/// scheduling cycle, so it takes neither [`CycleState`] nor [`NodeInfo`].
pub trait PreEnqueuePlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn pre_enqueue(&self, pod: &Pod) -> Status;
}

/// Upstream: `pkg/scheduler/framework/interface.go::QueueSortPlugin`.
///
/// Provides the strict-weak ordering the active queue pops in: `less(a, b)`
/// returns `true` when `a` should be scheduled before `b`. Exactly one
/// QueueSort plugin is enabled per profile (upstream enforces this); the
/// built-in [`PrioritySort`](crate::plugins::PrioritySort) orders by descending
/// `Pod.Spec.Priority`, ties broken by admission order (handled by the queue's
/// sequence number, not the plugin).
pub trait QueueSortPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    /// `true` iff `a` sorts before `b` (pops first).
    fn less(&self, a: &Pod, b: &Pod) -> bool;
}

/// Upstream: `pkg/scheduler/framework/interface.go::PreFilterExtensions`.
///
/// The optional incremental-update hook a PreFilter plugin exposes so the
/// preemption machinery can evaluate a node *as if* a victim pod were removed
/// (`remove_pod`) or an additional pod were added (`add_pod`) without recomputing
/// the whole pod-level state from scratch. The plugin mutates its own slice of
/// [`CycleState`] in place. Returning a non-success [`Status`] aborts the
/// hypothetical evaluation.
pub trait PreFilterExtensions: Send + Sync {
    /// A pod is being added to `node` in the hypothetical; update cycle state.
    fn add_pod(&self, state: &mut CycleState, pod: &Pod, pod_to_add: &Pod, node: &NodeInfo)
        -> Status;
    /// A pod is being removed from `node` in the hypothetical; update cycle state.
    fn remove_pod(
        &self,
        state: &mut CycleState,
        pod: &Pod,
        pod_to_remove: &Pod,
        node: &NodeInfo,
    ) -> Status;
}

/// Upstream: `pkg/scheduler/framework/interface.go::PreFilterPlugin`.
///
/// Runs once per pod before the per-node Filter loop: it can precompute
/// pod-level state (cached in [`CycleState`]) and optionally narrow the
/// candidate node set, or declare the pod outright unschedulable.
pub trait PreFilterPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn pre_filter(&self, state: &mut CycleState, pod: &Pod) -> (Option<PreFilterResult>, Status);

    /// Upstream: `PreFilterPlugin.PreFilterExtensions()`.
    ///
    /// Returns the plugin's incremental add/remove-pod hook, or `None` if it
    /// does not support hypothetical node mutation (the common case). The
    /// preemption machinery calls this to keep each plugin's cycle state
    /// consistent while it removes victim pods from a candidate node.
    fn pre_filter_extensions(&self) -> Option<&dyn PreFilterExtensions> {
        None
    }
}

/// Upstream: `pkg/scheduler/framework/interface.go::FilterPlugin`.
pub trait FilterPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn filter(&self, state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> Status;
}

/// Upstream: `pkg/scheduler/framework/interface.go::PreScorePlugin`.
///
/// Runs once per pod after Filter and before the per-node Score loop, over the
/// feasible node set; precomputes scoring state into [`CycleState`].
pub trait PreScorePlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn pre_score(&self, state: &mut CycleState, pod: &Pod, nodes: &[NodeInfo]) -> Status;
}

/// Upstream: `pkg/scheduler/framework/interface.go::ScorePlugin` (+ the optional
/// `ScoreExtensions.NormalizeScore`).
pub trait ScorePlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn score(&self, state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> (i64, Status);

    /// Upstream: `pkg/scheduler/framework/interface.go::ScoreExtensions.NormalizeScore`.
    ///
    /// Runs once after [`score`](Self::score) has produced a raw score for every
    /// feasible node, with the full `(node, raw-score)` set in hand, so the
    /// plugin can rescale it (e.g. spread to `[MIN_NODE_SCORE, MAX_NODE_SCORE]`)
    /// before the framework applies [`weight`](Self::weight). The default leaves
    /// scores untouched — a plugin that already emits in-range scores opts out by
    /// not overriding this. Mutates `scores` in place; a non-success [`Status`]
    /// aborts the scheduling cycle.
    fn normalize_score(
        &self,
        _state: &mut CycleState,
        _pod: &Pod,
        _scores: &mut [NodeScore],
    ) -> Status {
        Status::success()
    }

    /// Default weight applied to the produced score.
    /// Upstream: `pkg/scheduler/apis/config/types.go::Plugin.Weight`.
    fn weight(&self) -> i64 {
        1
    }
}

/// Upstream: `pkg/scheduler/framework/interface.go::PostFilterPlugin`.
/// Used by Phase 2 only for preemption.
pub trait PostFilterPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    /// Return the nominated node name (`Some(node)` on success) or `None`
    /// if no nomination could be made.
    fn post_filter(
        &self,
        state: &mut CycleState,
        pod: &Pod,
        nodes: &[NodeInfo],
        filter_failures: &FilterFailureMap,
    ) -> (Option<String>, Status);
}

/// Upstream: `pkg/scheduler/framework/interface.go::ReservePlugin`.
///
/// Runs in the binding cycle after a node is chosen: `reserve` claims runtime
/// resources for the pod on that node; `unreserve` rolls the claim back if any
/// later stage (Permit, PreBind, Bind) fails. Every Reserve that ran must be
/// Unreserved on failure, in reverse order.
pub trait ReservePlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn reserve(&self, state: &mut CycleState, pod: &Pod, node_name: &str) -> Status;
    fn unreserve(&self, state: &mut CycleState, pod: &Pod, node_name: &str);
}

/// Upstream: `pkg/scheduler/framework/interface.go::PermitPlugin`.
///
/// Gates whether the reserved pod may proceed to bind. A plugin returns one of
/// three dispositions:
///
/// * [`Code::Success`] — approve immediately, the next Permit plugin runs.
/// * a non-success, non-[`Code::Wait`] status (e.g. [`Status::unschedulable`]) —
///   reject; the framework Unreserves every Reserve plugin and re-queues the pod.
/// * [`Status::wait`] together with a [`Duration`](std::time::Duration) timeout —
///   hold the pod. The framework parks the pod (it does **not** bind) until the
///   plugin (or another plugin) calls [`WaitingPod::allow`] /
///   [`WaitingPod::reject`], or the *shortest* requested timeout elapses (which
///   the framework treats as a rejection). When a plugin returns
///   [`Code::Wait`] it must also report its timeout via
///   [`permit_timeout`](Self::permit_timeout).
pub trait PermitPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn permit(&self, state: &mut CycleState, pod: &Pod, node_name: &str) -> Status;

    /// The timeout for this plugin's [`Code::Wait`] disposition. The framework
    /// waits at most the minimum timeout across all waiting plugins; a plugin
    /// that never returns [`Code::Wait`] can leave this at its default of
    /// [`Duration::ZERO`], which is ignored. Upstream: the `timeout` returned
    /// alongside `framework.Wait` from `Permit`.
    fn permit_timeout(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}

/// Upstream: `pkg/scheduler/framework/runtime/waiting_pods_map.go::waitingPod`.
///
/// The gate the framework hands to whatever decides a waiting pod's fate. While
/// a pod is in the [`Code::Wait`] state, callers (e.g. a sibling Permit plugin
/// reacting to a cluster event) call [`allow`](Self::allow) to let it proceed to
/// bind, or [`reject`](Self::reject) to fail it. The framework's own timeout
/// fires a `reject` if neither is called in time. The signal is single-shot:
/// the first disposition wins and later calls are ignored.
#[derive(Clone)]
pub struct WaitingPod {
    pod_uid: String,
    inner: std::sync::Arc<WaitingPodInner>,
}

struct WaitingPodInner {
    tx: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<PermitDecision>>>,
}

/// The terminal disposition of a [`WaitingPod`]. Upstream: the `s *Status` the
/// `waitingPod`'s channel carries — `nil` for allow, non-`nil` for reject.
#[derive(Debug, Clone)]
pub enum PermitDecision {
    Allow,
    Reject(String),
}

impl WaitingPod {
    /// Construct a waiting-pod handle plus the receiver the framework awaits.
    #[must_use]
    pub fn new(pod_uid: impl Into<String>) -> (Self, tokio::sync::oneshot::Receiver<PermitDecision>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let wp = Self {
            pod_uid: pod_uid.into(),
            inner: std::sync::Arc::new(WaitingPodInner {
                tx: tokio::sync::Mutex::new(Some(tx)),
            }),
        };
        (wp, rx)
    }

    /// The uid of the pod this gate controls.
    #[must_use]
    pub fn pod_uid(&self) -> &str {
        &self.pod_uid
    }

    fn signal(&self, decision: PermitDecision) -> bool {
        // try_lock never blocks; the lock is uncontended in practice (the only
        // writer is whoever resolves the gate first) and a contended lock means
        // someone else is mid-resolution, so dropping the signal is correct.
        if let Ok(mut guard) = self.inner.tx.try_lock() {
            if let Some(tx) = guard.take() {
                return tx.send(decision).is_ok();
            }
        }
        false
    }

    /// Let the waiting pod proceed to bind. Returns `false` if the pod was
    /// already resolved (allowed, rejected, or timed out).
    pub fn allow(&self) -> bool {
        self.signal(PermitDecision::Allow)
    }

    /// Fail the waiting pod with `reason`; the framework Unreserves and
    /// re-queues it. Returns `false` if already resolved.
    pub fn reject(&self, reason: impl Into<String>) -> bool {
        self.signal(PermitDecision::Reject(reason.into()))
    }
}

/// Upstream: `pkg/scheduler/framework/interface.go::PreBindPlugin`.
///
/// Last hook before the bind RPC — e.g. provisioning a volume. A failure here
/// triggers Unreserve and a re-queue.
pub trait PreBindPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn pre_bind(&self, state: &mut CycleState, pod: &Pod, node_name: &str) -> Status;
}

/// Upstream: `pkg/scheduler/framework/interface.go::BindPlugin`.
///
/// The terminal binding-cycle extension point: it writes the pod→node
/// assignment (upstream POSTs a `core/v1.Binding`). Bind plugins run in
/// registration order; the first to return a non-[`Code::Skip`] status owns the
/// bind — its [`Status`] decides success or failure and no later bind plugin is
/// consulted. A plugin returns [`Code::Skip`] to abstain (it does not handle
/// this pod). If every bind plugin skips, or none is registered, the scheduler
/// falls back to its built-in `DefaultBinder` (the `SchedulerSink::bind` POST).
///
/// Bind is the one inherently-I/O extension point, so — unlike the CPU-only
/// Filter/Score/Reserve/Permit/PreBind traits — it is `async`.
#[async_trait::async_trait]
pub trait BindPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    async fn bind(&self, state: &mut CycleState, pod: &Pod, node_name: &str) -> Status;
}

/// Upstream: `pkg/scheduler/framework/interface.go::PostBindPlugin`.
///
/// Best-effort callback after a pod is successfully bound — e.g. to release
/// cycle-scoped state or emit a notification. It cannot fail the cycle (the bind
/// already happened) so it returns nothing, and it runs only on the success
/// path.
pub trait PostBindPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn post_bind(&self, state: &mut CycleState, pod: &Pod, node_name: &str);
}

/// Per-pod, per-node filter result map.
/// Upstream: `pkg/scheduler/framework/types.go::NodeToStatusMap`.
pub type FilterFailureMap = std::collections::BTreeMap<String, Status>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_success_is_treated_as_success() {
        assert!(Status::success().is_success());
    }

    #[test]
    fn status_unschedulable_has_reason() {
        let s = Status::unschedulable("Plug", "out of CPU");
        assert!(!s.is_success());
        assert_eq!(s.message(), "out of CPU");
        assert_eq!(s.plugin, "Plug");
    }

    #[test]
    fn min_max_node_score_constants_match_upstream() {
        assert_eq!(MIN_NODE_SCORE, 0);
        assert_eq!(MAX_NODE_SCORE, 100);
    }
}
