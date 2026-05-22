// SPDX-License-Identifier: Apache-2.0
//! `NodeResourcesBalancedAllocation` — favour nodes whose CPU/memory
//! utilisation ratios are balanced.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/noderesources/balanced_allocation.go

use crate::cache::NodeInfo;
use crate::framework::{CycleState, ScorePlugin, Status, MAX_NODE_SCORE};
use crate::types::{Pod, ResourceName};

pub struct NodeResourcesBalancedAllocation;

impl ScorePlugin for NodeResourcesBalancedAllocation {
    fn name(&self) -> &'static str {
        "NodeResourcesBalancedAllocation"
    }

    /// Upstream formula (`balanced_allocation.go::computeScore`):
    ///     fraction_i = (used_i + requested_i) / capacity_i
    ///     mean       = avg(fraction_i)
    ///     variance   = avg((fraction_i - mean)^2)
    ///     score      = (1 - variance) * 100
    ///
    /// Phase 2 considers only CPU and memory (matching `BalancedResources`
    /// default). Pod requests are added to the node's existing usage to
    /// score the *post-placement* world.
    fn score(&self, _state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> (i64, Status) {
        let resources = [ResourceName::Cpu, ResourceName::Memory];
        let mut fractions = Vec::with_capacity(resources.len());

        for r in resources {
            let cap = node.allocatable(r).value();
            if cap == 0 {
                continue;
            }
            let used = node.requested(r);
            let req = pod.sum_requests(r).value();
            // Upstream uses float ratios; we scale by 10000 to stay in i64.
            let total = used.saturating_add(req);
            let frac = (total.saturating_mul(10_000)) / cap;
            fractions.push(frac.clamp(0, 10_000));
        }
        if fractions.is_empty() {
            return (0, Status::success());
        }
        let mean: i64 = fractions.iter().sum::<i64>() / fractions.len() as i64;
        let var: i64 = fractions
            .iter()
            .map(|f| {
                let d = (*f - mean).abs();
                d.saturating_mul(d) / 10_000
            })
            .sum::<i64>()
            / fractions.len() as i64;
        // variance is in [0, 10_000]; map to score = (10_000 - variance) * 100 / 10_000.
        let score = ((10_000_i64 - var) * MAX_NODE_SCORE) / 10_000;
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

    fn node(cap_cpu: i64, cap_mem: i64) -> Node {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cap_cpu));
        n.status
            .allocatable
            .insert(ResourceName::Memory, Quantity::bytes(cap_mem));
        n
    }

    fn pod(cpu_m: i64, mem_b: i64) -> Pod {
        let mut p = Pod::default();
        let mut c = Container::default();
        if cpu_m > 0 {
            c.resources
                .requests
                .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
        }
        if mem_b > 0 {
            c.resources
                .requests
                .insert(ResourceName::Memory, Quantity::bytes(mem_b));
        }
        p.spec.containers.push(c);
        p
    }

    #[test]
    fn balanced_usage_scores_near_max() {
        let info = NodeInfo::new(node(1000, 1024));
        // Placing 500/512 → 50%/50% fractions, variance 0 → 100.
        let p = pod(500, 512);
        let mut s = CycleState::new();
        let (sc, _) = NodeResourcesBalancedAllocation.score(&mut s, &p, &info);
        assert_eq!(sc, MAX_NODE_SCORE);
    }

    #[test]
    fn imbalanced_usage_scores_lower() {
        let info = NodeInfo::new(node(1000, 1024));
        // Placing 900/100 → very skewed → lower score.
        let p = pod(900, 100);
        let mut s = CycleState::new();
        let (sc, _) = NodeResourcesBalancedAllocation.score(&mut s, &p, &info);
        assert!(sc < MAX_NODE_SCORE);
        assert!(sc >= 0);
    }
}
