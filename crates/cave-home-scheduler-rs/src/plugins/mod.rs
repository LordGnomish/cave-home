// SPDX-License-Identifier: Apache-2.0
//! Default-profile plugin implementations.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/...

pub mod image_locality;
pub mod least_requested;
pub mod node_name;
pub mod node_ports;
pub mod node_resources_balanced;
pub mod node_resources_fit;
pub mod node_unschedulable;
pub mod taint_toleration;
pub mod volume_restrictions;

pub use image_locality::ImageLocality;
pub use least_requested::LeastRequested;
pub use node_name::NodeName;
pub use node_ports::NodePorts;
pub use node_resources_balanced::NodeResourcesBalancedAllocation;
pub use node_resources_fit::NodeResourcesFit;
pub use node_unschedulable::NodeUnschedulable;
pub use taint_toleration::TaintToleration;
pub use volume_restrictions::VolumeRestrictions;

use std::sync::Arc;

use crate::framework::{PluginRegistry, RegistryBuilder};

/// Upstream: `pkg/scheduler/apis/config/v1/default_plugins.go::getDefaultPlugins`.
#[must_use]
pub fn default_registry() -> PluginRegistry {
    RegistryBuilder::default()
        // PreFilter — NodeResourcesFit precomputes the pod's summed requests
        // once so the per-node Filter loop reuses them (upstream default).
        .with_pre_filter(Arc::new(NodeResourcesFit))
        // Filters — order chosen to mirror upstream defaults file
        // (NodeUnschedulable runs before NodeName so a tainted control-plane
        // returns the cheaper "Unschedulable" reason).
        .with_filter(Arc::new(NodeUnschedulable))
        .with_filter(Arc::new(NodeName))
        .with_filter(Arc::new(NodeResourcesFit))
        .with_filter(Arc::new(NodePorts))
        .with_filter(Arc::new(VolumeRestrictions))
        .with_filter(Arc::new(TaintToleration))
        .with_filter(Arc::new(node_affinity_required_filter()))
        // Scores
        .with_score(Arc::new(NodeResourcesBalancedAllocation))
        .with_score(Arc::new(LeastRequested))
        .with_score(Arc::new(ImageLocality))
        // Preemption (post-filter)
        .with_post_filter(Arc::new(crate::preemption::DefaultPreemption::new()))
        .build()
}

/// Synthetic helper: NodeAffinity required-during-scheduling filter.
fn node_affinity_required_filter() -> impl crate::framework::FilterPlugin {
    NodeAffinityFilter
}

/// Upstream: `pkg/scheduler/framework/plugins/nodeaffinity/node_affinity.go::Filter`.
/// Phase 2 only honours `RequiredDuringSchedulingIgnoredDuringExecution` and
/// `Pod.Spec.NodeSelector` (PreferredDuringScheduling is deferred).
pub struct NodeAffinityFilter;

impl crate::framework::FilterPlugin for NodeAffinityFilter {
    fn name(&self) -> &'static str {
        "NodeAffinity"
    }

    fn filter(
        &self,
        _state: &mut crate::framework::CycleState,
        pod: &crate::types::Pod,
        node: &crate::cache::NodeInfo,
    ) -> crate::framework::Status {
        let labels = &node.node().metadata.labels;

        // Pod.Spec.NodeSelector — every key must be present and equal.
        for (k, v) in &pod.spec.node_selector {
            match labels.get(k) {
                Some(existing) if existing == v => {}
                _ => {
                    return crate::framework::Status::unresolvable(
                        self.name(),
                        format!("nodeSelector mismatch on label {k}"),
                    );
                }
            }
        }

        // Required node affinity.
        if let Some(aff) = pod.spec.affinity.as_ref() {
            if let Some(na) = aff.node_affinity.as_ref() {
                if let Some(req) = na.required_during_scheduling.as_ref() {
                    if !req.matches(labels) {
                        return crate::framework::Status::unresolvable(
                            self.name(),
                            "required node affinity does not match",
                        );
                    }
                }
            }
        }

        crate::framework::Status::success()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::framework::{CycleState, FilterPlugin};
    use crate::types::{
        Affinity, Node, NodeAffinity, NodeSelector, NodeSelectorOperator, NodeSelectorRequirement,
        NodeSelectorTerm, Pod,
    };

    fn node_with_labels(pairs: &[(&str, &str)]) -> Node {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        for (k, v) in pairs {
            n.metadata.labels.insert((*k).into(), (*v).into());
        }
        n
    }

    #[test]
    fn default_registry_lists_all_phase2_plugins() {
        let r = default_registry();
        assert_eq!(r.filters().len(), 7);
        assert_eq!(r.scores().len(), 3);
        assert_eq!(r.post_filters().len(), 1);
    }

    #[test]
    fn node_affinity_matches_required_terms() {
        let n = node_with_labels(&[("zone", "us-east")]);
        let info = NodeInfo::new(n);
        let mut p = Pod::default();
        p.spec.affinity = Some(Affinity {
            node_affinity: Some(NodeAffinity {
                required_during_scheduling: Some(NodeSelector {
                    node_selector_terms: vec![NodeSelectorTerm {
                        match_expressions: vec![NodeSelectorRequirement {
                            key: "zone".into(),
                            operator: Some(NodeSelectorOperator::In),
                            values: vec!["us-east".into()],
                        }],
                    }],
                }),
            }),
        });
        let mut s = CycleState::new();
        assert!(NodeAffinityFilter.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn node_affinity_rejects_non_matching_term() {
        let n = node_with_labels(&[("zone", "us-west")]);
        let info = NodeInfo::new(n);
        let mut p = Pod::default();
        p.spec.affinity = Some(Affinity {
            node_affinity: Some(NodeAffinity {
                required_during_scheduling: Some(NodeSelector {
                    node_selector_terms: vec![NodeSelectorTerm {
                        match_expressions: vec![NodeSelectorRequirement {
                            key: "zone".into(),
                            operator: Some(NodeSelectorOperator::In),
                            values: vec!["us-east".into()],
                        }],
                    }],
                }),
            }),
        });
        let mut s = CycleState::new();
        let st = NodeAffinityFilter.filter(&mut s, &p, &info);
        assert!(!st.is_success());
    }

    #[test]
    fn pod_node_selector_must_match_all_labels() {
        let n = node_with_labels(&[("a", "1")]);
        let info = NodeInfo::new(n);
        let mut p = Pod::default();
        p.spec.node_selector.insert("a".into(), "1".into());
        p.spec.node_selector.insert("b".into(), "2".into());
        let mut s = CycleState::new();
        let st = NodeAffinityFilter.filter(&mut s, &p, &info);
        assert!(!st.is_success());
    }
}
