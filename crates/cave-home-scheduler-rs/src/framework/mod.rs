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

/// Upstream: `pkg/scheduler/framework/interface.go::PreFilterPlugin`.
///
/// Runs once per pod before the per-node Filter loop: it can precompute
/// pod-level state (cached in [`CycleState`]) and optionally narrow the
/// candidate node set, or declare the pod outright unschedulable.
pub trait PreFilterPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn pre_filter(&self, state: &mut CycleState, pod: &Pod) -> (Option<PreFilterResult>, Status);
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
/// Gates whether the reserved pod may proceed to bind. Phase 2 supports
/// approve (`Success`) or deny (`Unschedulable`); the timed "wait" disposition
/// is deferred (see `parity.manifest.toml`).
pub trait PermitPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn permit(&self, state: &mut CycleState, pod: &Pod, node_name: &str) -> Status;
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
