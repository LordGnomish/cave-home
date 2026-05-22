// SPDX-License-Identifier: Apache-2.0
//! `NodeUnschedulable` — respect `Node.Spec.Unschedulable`.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/nodeunschedulable/node_unschedulable.go

use crate::cache::NodeInfo;
use crate::framework::{CycleState, FilterPlugin, Status};
use crate::types::{Pod, TaintEffect, TolerationOperator};

pub struct NodeUnschedulable;

/// Upstream: `v1.TaintNodeUnschedulable`.
pub const TAINT_NODE_UNSCHEDULABLE: &str = "node.kubernetes.io/unschedulable";

impl FilterPlugin for NodeUnschedulable {
    fn name(&self) -> &'static str {
        "NodeUnschedulable"
    }

    fn filter(&self, _state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> Status {
        if !node.node().spec.unschedulable {
            return Status::success();
        }
        // Upstream allows the pod through if it tolerates the synthetic
        // `node.kubernetes.io/unschedulable:NoSchedule` taint (Exists op).
        let tolerated = pod.spec.tolerations.iter().any(|t| {
            matches!(t.operator, TolerationOperator::Exists)
                && (t.key.is_empty() || t.key == TAINT_NODE_UNSCHEDULABLE)
                && t.effect.is_none_or(|e| e == TaintEffect::NoSchedule)
        });
        if tolerated {
            Status::success()
        } else {
            Status::unresolvable(self.name(), "node(s) were unschedulable")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Node, Pod, TaintEffect, Toleration, TolerationOperator};

    fn node(unsched: bool) -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        n.spec.unschedulable = unsched;
        NodeInfo::new(n)
    }

    #[test]
    fn schedulable_node_passes() {
        let info = node(false);
        let p = Pod::default();
        let mut s = CycleState::new();
        assert!(NodeUnschedulable.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn unschedulable_node_fails_without_toleration() {
        let info = node(true);
        let p = Pod::default();
        let mut s = CycleState::new();
        assert!(!NodeUnschedulable.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn pod_with_existence_toleration_for_unschedulable_passes() {
        let info = node(true);
        let mut p = Pod::default();
        p.spec.tolerations.push(Toleration {
            key: TAINT_NODE_UNSCHEDULABLE.into(),
            operator: TolerationOperator::Exists,
            value: String::new(),
            effect: Some(TaintEffect::NoSchedule),
        });
        let mut s = CycleState::new();
        assert!(NodeUnschedulable.filter(&mut s, &p, &info).is_success());
    }
}
