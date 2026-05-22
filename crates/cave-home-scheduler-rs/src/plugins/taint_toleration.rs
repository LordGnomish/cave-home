// SPDX-License-Identifier: Apache-2.0
//! `TaintToleration` — reject nodes whose `NoSchedule` taints lack a toleration.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/tainttoleration/taint_toleration.go

use crate::cache::NodeInfo;
use crate::framework::{CycleState, FilterPlugin, Status};
use crate::types::{Pod, TaintEffect};

pub struct TaintToleration;

impl FilterPlugin for TaintToleration {
    fn name(&self) -> &'static str {
        "TaintToleration"
    }

    fn filter(&self, _state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> Status {
        for taint in &node.node().spec.taints {
            // Phase 2 only enforces NoSchedule; NoExecute is an eviction
            // signal, PreferNoSchedule is advisory.
            if taint.effect != TaintEffect::NoSchedule {
                continue;
            }
            let tolerated = pod.spec.tolerations.iter().any(|t| t.tolerates(taint));
            if !tolerated {
                return Status::unschedulable(
                    self.name(),
                    format!(
                        "node(s) had untolerated taint {{{}={}:{:?}}}",
                        taint.key, taint.value, taint.effect
                    ),
                );
            }
        }
        Status::success()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Node, Pod, Taint, TaintEffect, Toleration, TolerationOperator};

    fn node_with_taint(t: Taint) -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        n.spec.taints.push(t);
        NodeInfo::new(n)
    }

    #[test]
    fn untainted_node_passes() {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        let info = NodeInfo::new(n);
        let p = Pod::default();
        let mut s = CycleState::new();
        assert!(TaintToleration.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn no_schedule_taint_blocks_pod_without_toleration() {
        let info = node_with_taint(Taint {
            key: "k".into(),
            value: "v".into(),
            effect: TaintEffect::NoSchedule,
        });
        let p = Pod::default();
        let mut s = CycleState::new();
        assert!(!TaintToleration.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn matching_toleration_allows_pod_on_taint() {
        let info = node_with_taint(Taint {
            key: "k".into(),
            value: "v".into(),
            effect: TaintEffect::NoSchedule,
        });
        let mut p = Pod::default();
        p.spec.tolerations.push(Toleration {
            key: "k".into(),
            operator: TolerationOperator::Equal,
            value: "v".into(),
            effect: Some(TaintEffect::NoSchedule),
        });
        let mut s = CycleState::new();
        assert!(TaintToleration.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn prefer_no_schedule_taint_does_not_block() {
        let info = node_with_taint(Taint {
            key: "k".into(),
            value: "v".into(),
            effect: TaintEffect::PreferNoSchedule,
        });
        let p = Pod::default();
        let mut s = CycleState::new();
        assert!(TaintToleration.filter(&mut s, &p, &info).is_success());
    }
}
