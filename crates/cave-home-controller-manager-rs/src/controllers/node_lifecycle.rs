// SPDX-License-Identifier: Apache-2.0
//! Node lifecycle controller — health classification, taints and eviction.
//!
//! Behavioural reimplementation of the documented node-lifecycle-controller
//! contract (`pkg/controller/nodelifecycle`): classify each node from its
//! `Ready` condition and heartbeat age, manage the `not-ready` /
//! `unreachable` `NoExecute` taints, and trigger pod eviction once a node has
//! been unhealthy past a grace period. `std` only; time is a caller-supplied
//! `now` (epoch seconds), never a clock read.
//!
//! Classification rules (each tested):
//! * `Ready=True` and a fresh heartbeat -> [`NodeHealth::Ready`];
//! * `Ready=False` (kubelet says not ready) -> [`NodeHealth::NotReady`];
//! * `Ready=Unknown` **or** heartbeat older than `monitor_grace` (kubelet
//!   stopped reporting) -> [`NodeHealth::Unreachable`].
//!
//! The well-known taint keys mirror upstream:
//! `node.kubernetes.io/not-ready` and `node.kubernetes.io/unreachable`.

/// The standard `NoExecute` taint applied to a `NotReady` node.
pub const TAINT_NOT_READY: &str = "node.kubernetes.io/not-ready";
/// The standard `NoExecute` taint applied to an `Unreachable` node.
pub const TAINT_UNREACHABLE: &str = "node.kubernetes.io/unreachable";

/// The kubelet `Ready` condition status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyCondition {
    /// kubelet reports the node healthy.
    True,
    /// kubelet reports the node unhealthy.
    False,
    /// kubelet has not reported a definite status.
    Unknown,
}

/// Health classification of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeHealth {
    /// Healthy and reporting.
    Ready,
    /// Reporting, but unhealthy (`Ready=False`).
    NotReady,
    /// Not reporting — stale heartbeat or `Ready=Unknown`.
    Unreachable,
}

/// Observed node state handed to the controller.
#[derive(Debug, Clone)]
pub struct NodeState {
    /// Node name.
    pub name: String,
    /// kubelet `Ready` condition.
    pub ready: ReadyCondition,
    /// Epoch-seconds of the last heartbeat (`Ready` condition's
    /// `lastHeartbeatTime`).
    pub last_heartbeat: i64,
    /// Taints currently present on the node, by key.
    pub taints: Vec<String>,
}

impl NodeState {
    /// A healthy node reporting now.
    #[must_use]
    pub fn ready(name: &str, last_heartbeat: i64) -> Self {
        Self {
            name: name.to_owned(),
            ready: ReadyCondition::True,
            last_heartbeat,
            taints: Vec::new(),
        }
    }
}

/// Timing knobs (caller's clock unit; seconds in the docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleConfig {
    /// Heartbeats older than this mark a node `Unreachable`
    /// (upstream `node-monitor-grace-period`, default 40 s).
    pub monitor_grace: i64,
    /// After a node has been unhealthy for this long, start evicting its pods
    /// (upstream `pod-eviction-timeout`, default 300 s).
    pub eviction_grace: i64,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            monitor_grace: 40,
            eviction_grace: 300,
        }
    }
}

/// One action the controller decides to take for a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeAction {
    /// Add the named taint (it was absent).
    AddTaint(String),
    /// Remove the named taint (it was present but the node recovered).
    RemoveTaint(String),
    /// Begin evicting this node's pods — it has been unhealthy past the grace
    /// period. Carries the node name.
    EvictPods(String),
}

/// Classify a node from its `Ready` condition and heartbeat age.
#[must_use]
pub const fn classify(node: &NodeState, cfg: &LifecycleConfig, now: i64) -> NodeHealth {
    let stale = now.saturating_sub(node.last_heartbeat) > cfg.monitor_grace;
    match node.ready {
        ReadyCondition::Unknown => NodeHealth::Unreachable,
        _ if stale => NodeHealth::Unreachable,
        ReadyCondition::True => NodeHealth::Ready,
        ReadyCondition::False => NodeHealth::NotReady,
    }
}

/// The taint key implied by a health state, if any.
#[must_use]
const fn taint_for(health: NodeHealth) -> Option<&'static str> {
    match health {
        NodeHealth::Ready => None,
        NodeHealth::NotReady => Some(TAINT_NOT_READY),
        NodeHealth::Unreachable => Some(TAINT_UNREACHABLE),
    }
}

/// Decide the actions for one node: reconcile its taints to match its health,
/// and trigger eviction once it has been unhealthy past `eviction_grace`.
///
/// `unhealthy_since` is when the node *first* became unhealthy (caller tracks
/// this across reconciles); `None` means it is currently healthy. Eviction is
/// triggered only while the node carries its unhealthy taint **and** has been
/// unhealthy for at least `eviction_grace`.
#[must_use]
pub fn reconcile_node(
    node: &NodeState,
    cfg: &LifecycleConfig,
    unhealthy_since: Option<i64>,
    now: i64,
) -> Vec<NodeAction> {
    let mut actions = Vec::new();
    let health = classify(node, cfg, now);
    let wanted = taint_for(health);

    // Remove any unhealthy taint that no longer applies.
    for present in [TAINT_NOT_READY, TAINT_UNREACHABLE] {
        let has = node.taints.iter().any(|t| t == present);
        if has && wanted != Some(present) {
            actions.push(NodeAction::RemoveTaint(present.to_owned()));
        }
    }
    // Add the taint the current health calls for, if absent.
    if let Some(key) = wanted {
        if !node.taints.iter().any(|t| t == key) {
            actions.push(NodeAction::AddTaint(key.to_owned()));
        }
    }

    // Eviction: unhealthy long enough.
    if health != NodeHealth::Ready {
        if let Some(since) = unhealthy_since {
            if now.saturating_sub(since) >= cfg.eviction_grace {
                actions.push(NodeAction::EvictPods(node.name.clone()));
            }
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> LifecycleConfig {
        LifecycleConfig { monitor_grace: 40, eviction_grace: 300 }
    }

    #[test]
    fn fresh_ready_node_is_ready() {
        let n = NodeState::ready("n", 100);
        assert_eq!(classify(&n, &cfg(), 120), NodeHealth::Ready);
    }

    #[test]
    fn ready_false_is_not_ready() {
        let mut n = NodeState::ready("n", 100);
        n.ready = ReadyCondition::False;
        assert_eq!(classify(&n, &cfg(), 110), NodeHealth::NotReady);
    }

    #[test]
    fn ready_unknown_is_unreachable() {
        let mut n = NodeState::ready("n", 100);
        n.ready = ReadyCondition::Unknown;
        assert_eq!(classify(&n, &cfg(), 110), NodeHealth::Unreachable);
    }

    #[test]
    fn stale_heartbeat_is_unreachable_even_if_last_ready_true() {
        let n = NodeState::ready("n", 100); // Ready=True but old heartbeat
        // now - last = 100 > grace 40
        assert_eq!(classify(&n, &cfg(), 200), NodeHealth::Unreachable);
    }

    #[test]
    fn heartbeat_exactly_at_grace_is_still_ready() {
        let n = NodeState::ready("n", 100);
        // now - last = 40, NOT > 40
        assert_eq!(classify(&n, &cfg(), 140), NodeHealth::Ready);
    }

    #[test]
    fn ready_node_with_stale_taint_gets_taint_removed() {
        let mut n = NodeState::ready("n", 100);
        n.taints.push(TAINT_NOT_READY.to_owned());
        let actions = reconcile_node(&n, &cfg(), None, 120);
        assert_eq!(actions, vec![NodeAction::RemoveTaint(TAINT_NOT_READY.to_owned())]);
    }

    #[test]
    fn not_ready_node_gets_not_ready_taint_added() {
        let mut n = NodeState::ready("n", 100);
        n.ready = ReadyCondition::False;
        let actions = reconcile_node(&n, &cfg(), Some(100), 110);
        assert!(actions.contains(&NodeAction::AddTaint(TAINT_NOT_READY.to_owned())));
    }

    #[test]
    fn unreachable_node_gets_unreachable_taint_and_swaps_not_ready() {
        let mut n = NodeState::ready("n", 100);
        n.ready = ReadyCondition::Unknown;
        n.taints.push(TAINT_NOT_READY.to_owned()); // was previously not-ready
        let actions = reconcile_node(&n, &cfg(), Some(100), 110);
        assert!(actions.contains(&NodeAction::RemoveTaint(TAINT_NOT_READY.to_owned())));
        assert!(actions.contains(&NodeAction::AddTaint(TAINT_UNREACHABLE.to_owned())));
    }

    #[test]
    fn eviction_triggers_after_grace_period() {
        let mut n = NodeState::ready("n", 0);
        n.ready = ReadyCondition::Unknown;
        n.taints.push(TAINT_UNREACHABLE.to_owned());
        // unhealthy since 100, now 100+300 = 400 -> at grace
        let actions = reconcile_node(&n, &cfg(), Some(100), 400);
        assert!(actions.contains(&NodeAction::EvictPods("n".to_owned())));
    }

    #[test]
    fn eviction_does_not_trigger_before_grace_period() {
        let mut n = NodeState::ready("n", 0);
        n.ready = ReadyCondition::Unknown;
        n.taints.push(TAINT_UNREACHABLE.to_owned());
        // unhealthy since 100, now 100+299 -> just under grace
        let actions = reconcile_node(&n, &cfg(), Some(100), 399);
        assert!(!actions.iter().any(|a| matches!(a, NodeAction::EvictPods(_))));
    }

    #[test]
    fn ready_node_never_evicts() {
        let n = NodeState::ready("n", 1000);
        let actions = reconcile_node(&n, &cfg(), None, 1000);
        assert!(actions.is_empty());
    }
}
