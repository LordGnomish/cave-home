// SPDX-License-Identifier: Apache-2.0
//! Cluster-event vocabulary used by the scheduling queue to decide which
//! unschedulable pods are worth re-queueing after a cluster mutation.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/types.go (GVK, ActionType, ClusterEvent)

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
