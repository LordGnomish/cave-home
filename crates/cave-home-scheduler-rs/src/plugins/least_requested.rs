// SPDX-License-Identifier: Apache-2.0
//! `LeastRequested` — prefer the node with the most free CPU/memory.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/noderesources/least_allocated.go

use crate::cache::NodeInfo;
use crate::framework::{CycleState, ScorePlugin, Status, MAX_NODE_SCORE};
use crate::types::{Pod, ResourceName};

pub struct LeastRequested;

impl ScorePlugin for LeastRequested {
    fn name(&self) -> &'static str {
        "NodeResourcesLeastAllocated"
    }

    /// Upstream formula:
    ///     `((capacity - request) * 100) / capacity` per resource,
    ///     then averaged across resources.
    fn score(&self, _state: &mut CycleState, _pod: &Pod, node: &NodeInfo) -> (i64, Status) {
        let mut sum = 0_i64;
        let mut count = 0_i64;
        for r in [ResourceName::Cpu, ResourceName::Memory] {
            let cap = node.allocatable(r).value();
            if cap == 0 {
                continue;
            }
            let used = node.requested(r);
            let free = cap.saturating_sub(used).max(0);
            sum += (free.saturating_mul(MAX_NODE_SCORE)) / cap;
            count += 1;
        }
        let score = if count == 0 { 0 } else { sum / count };
        (score.clamp(0, MAX_NODE_SCORE), Status::success())
    }

    fn weight(&self) -> i64 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Container, Node, Pod, Quantity, ResourceName};

    fn node_with_load(cap_cpu: i64, used_cpu: i64) -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cap_cpu));
        n.status
            .allocatable
            .insert(ResourceName::Memory, Quantity::bytes(1024));
        let mut info = NodeInfo::new(n);
        if used_cpu > 0 {
            let mut p = Pod::default();
            let mut c = Container::default();
            c.resources
                .requests
                .insert(ResourceName::Cpu, Quantity::milli_cpu(used_cpu));
            p.spec.containers.push(c);
            info.add_pod(p);
        }
        info
    }

    #[test]
    fn empty_node_scores_at_max() {
        let info = node_with_load(1000, 0);
        let mut s = CycleState::new();
        let (sc, _) = LeastRequested.score(&mut s, &Pod::default(), &info);
        // Memory free 100%, CPU free 100% => 100.
        assert_eq!(sc, MAX_NODE_SCORE);
    }

    #[test]
    fn fully_loaded_cpu_lowers_score() {
        let busy = node_with_load(1000, 1000);
        let mut s = CycleState::new();
        let (sc_busy, _) = LeastRequested.score(&mut s, &Pod::default(), &busy);
        let idle = node_with_load(1000, 0);
        let (sc_idle, _) = LeastRequested.score(&mut s, &Pod::default(), &idle);
        assert!(sc_idle > sc_busy);
    }
}
