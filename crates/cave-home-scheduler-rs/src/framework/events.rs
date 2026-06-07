// SPDX-License-Identifier: Apache-2.0
//! Cluster-event vocabulary used by the scheduling queue to decide which
//! unschedulable pods are worth re-queueing after a cluster mutation.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/types.go (`GVK`, `ActionType`, `ClusterEvent`)

/// A `uint64` bitmask describing what happened to a cluster resource.
///
/// Upstream: `pkg/scheduler/framework/types.go::ActionType`. Plugins register
/// the action(s) that, when observed, could make a previously-unschedulable
/// pod schedulable, and the queue consults the mask to decide whether to move
/// a waiting pod back to the active queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionType(u64);

impl ActionType {
    /// Upstream: `framework.Add`.
    pub const ADD: Self = Self(1 << 0);
    /// Upstream: `framework.Delete`.
    pub const DELETE: Self = Self(1 << 1);
    /// Upstream: `framework.UpdateNodeAllocatable`.
    pub const UPDATE_NODE_ALLOCATABLE: Self = Self(1 << 2);
    /// Upstream: `framework.UpdateNodeLabel`.
    pub const UPDATE_NODE_LABEL: Self = Self(1 << 3);
    /// Upstream: `framework.UpdateNodeTaint`.
    pub const UPDATE_NODE_TAINT: Self = Self(1 << 4);
    /// Upstream: `framework.UpdateNodeCondition`.
    pub const UPDATE_NODE_CONDITION: Self = Self(1 << 5);
    /// Upstream: `framework.UpdatePodLabel` — assigned-pod label change.
    pub const UPDATE_POD_LABEL: Self = Self(1 << 6);

    /// Upstream: `framework.All` — the union of every specific action.
    pub const ALL: Self = Self(u64::MAX);

    /// Empty mask (no action).
    pub const NONE: Self = Self(0);

    /// True if `self` contains every bit of `other`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// True if `self` and `other` share at least one action bit.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for ActionType {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for ActionType {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Upstream: `pkg/scheduler/framework/types.go::GVK` — the resource kind a
/// `ClusterEvent` refers to. Phase 2 needs only the kinds the default plugin
/// set reacts to; `WildCard` matches any kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gvk {
    Pod,
    Node,
    /// Upstream: `framework.WildCard` — matches any resource kind.
    WildCard,
}

/// Upstream: `pkg/scheduler/framework/types.go::ClusterEvent`.
///
/// A registered event (what a pod is waiting for) is matched against an
/// occurred event (what the informer just saw). Both the resource kind and at
/// least one action bit must line up — modulo `WildCard`, which matches all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClusterEvent {
    pub resource: Gvk,
    pub action_type: ActionType,
}

impl ClusterEvent {
    #[must_use]
    pub const fn new(resource: Gvk, action_type: ActionType) -> Self {
        Self {
            resource,
            action_type,
        }
    }

    /// Upstream: `clusterEvent.Match` — does the `occurred` event satisfy the
    /// wake-up condition expressed by `self` (the registered event)?
    #[must_use]
    pub fn matches(&self, occurred: &Self) -> bool {
        let resource_ok = self.resource == Gvk::WildCard
            || occurred.resource == Gvk::WildCard
            || self.resource == occurred.resource;
        resource_ok && self.action_type.intersects(occurred.action_type)
    }
}

/// Upstream: `pkg/scheduler/framework/types.go::WildCardEvent` — the event
/// every pod is implicitly registered for, so any cluster change at least
/// considers waking it.
pub const WILD_CARD_EVENT: ClusterEvent = ClusterEvent {
    resource: Gvk::WildCard,
    action_type: ActionType::ALL,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_type_bitflags_compose() {
        let both = ActionType::ADD | ActionType::DELETE;
        assert!(both.contains(ActionType::ADD));
        assert!(both.contains(ActionType::DELETE));
        assert!(!both.contains(ActionType::UPDATE_NODE_TAINT));
    }

    #[test]
    fn all_action_contains_every_specific_action() {
        assert!(ActionType::ALL.contains(ActionType::ADD));
        assert!(ActionType::ALL.contains(ActionType::DELETE));
        assert!(ActionType::ALL.contains(ActionType::UPDATE_NODE_ALLOCATABLE));
        assert!(ActionType::ALL.contains(ActionType::UPDATE_NODE_TAINT));
    }

    #[test]
    fn cluster_event_matches_same_resource_and_overlapping_action() {
        // An unschedulable pod registered to wake on NodeAdd should match a
        // concrete NodeAdd event.
        let registered = ClusterEvent::new(Gvk::Node, ActionType::ADD | ActionType::UPDATE_NODE_TAINT);
        let occurred = ClusterEvent::new(Gvk::Node, ActionType::ADD);
        assert!(registered.matches(&occurred));
    }

    #[test]
    fn cluster_event_does_not_match_different_resource() {
        let registered = ClusterEvent::new(Gvk::Node, ActionType::ADD);
        let occurred = ClusterEvent::new(Gvk::Pod, ActionType::ADD);
        assert!(!registered.matches(&occurred));
    }

    #[test]
    fn cluster_event_does_not_match_disjoint_action() {
        let registered = ClusterEvent::new(Gvk::Node, ActionType::UPDATE_NODE_TAINT);
        let occurred = ClusterEvent::new(Gvk::Node, ActionType::ADD);
        assert!(!registered.matches(&occurred));
    }

    #[test]
    fn wildcard_event_matches_anything() {
        let occurred = ClusterEvent::new(Gvk::Node, ActionType::ADD);
        assert!(WILD_CARD_EVENT.matches(&occurred));
        let occurred2 = ClusterEvent::new(Gvk::Pod, ActionType::DELETE);
        assert!(WILD_CARD_EVENT.matches(&occurred2));
    }

    #[test]
    fn wildcard_resource_in_registration_matches_any_resource() {
        let registered = ClusterEvent::new(Gvk::WildCard, ActionType::ADD);
        assert!(registered.matches(&ClusterEvent::new(Gvk::Pod, ActionType::ADD)));
        assert!(registered.matches(&ClusterEvent::new(Gvk::Node, ActionType::ADD)));
    }
}
