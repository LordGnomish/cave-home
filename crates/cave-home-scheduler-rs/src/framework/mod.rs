// SPDX-License-Identifier: Apache-2.0
//! Scheduling framework — plugin trait surface.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/interface.go
//!
//! Phase 2 covers the three extension points the default plugin set
//! exercises: `FilterPlugin`, `ScorePlugin`, and `PostFilterPlugin`
//! (used by preemption). PreFilter / Reserve / Permit / PreBind /
//! PostBind are deferred — see `parity.manifest.toml`.

pub mod cycle_state;
pub mod events;
pub mod registry;

pub use cycle_state::CycleState;
pub use events::{ActionType, ClusterEvent, Gvk, WILD_CARD_EVENT};
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

/// Upstream: `pkg/scheduler/framework/interface.go::FilterPlugin`.
pub trait FilterPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn filter(&self, state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> Status;
}

/// Upstream: `pkg/scheduler/framework/interface.go::ScorePlugin`.
pub trait ScorePlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn score(&self, state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> (i64, Status);
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
