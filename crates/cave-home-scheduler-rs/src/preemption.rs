// SPDX-License-Identifier: Apache-2.0
//! Priority-based preemption — `DefaultPreemption` PostFilter plugin.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/defaultpreemption/default_preemption.go
//!
//! Phase 2 scope: find any node whose lower-priority pods, if all removed,
//! would let the incoming pod fit. Lower-priority-pod *minimisation*
//! (the upstream `selectVictimsOnNode` cost/regret optimisation) is
//! deferred — see `parity.manifest.toml`.

use crate::cache::NodeInfo;
use crate::framework::{
    CycleState, FilterFailureMap, FilterPlugin, MAX_NODE_SCORE, PostFilterPlugin, Status,
};
use crate::plugins::{
    NodeAffinityFilter, NodeName, NodePorts, NodeResourcesFit, NodeUnschedulable, TaintToleration,
    VolumeRestrictions,
};
use crate::types::Pod;

/// Upstream: `defaultpreemption.DefaultPreemption`.
#[derive(Default)]
pub struct DefaultPreemption;

impl DefaultPreemption {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Returns true if `pod` would fit on `info` once `victims` were removed.
    fn fits_after_eviction(pod: &Pod, info: &NodeInfo, victims: &[Pod]) -> bool {
        // Build a synthetic NodeInfo where victims are gone.
        let mut hypo = info.clone();
        for v in victims {
            hypo.remove_pod(v);
        }
        let mut state = CycleState::new();
        // Re-run the cheap filters that respect mutated NodeInfo state.
        let filters: Vec<Box<dyn FilterPlugin>> = vec![
            Box::new(NodeUnschedulable),
            Box::new(NodeName),
            Box::new(NodeResourcesFit),
            Box::new(NodePorts),
            Box::new(VolumeRestrictions),
            Box::new(TaintToleration),
            Box::new(NodeAffinityFilter),
        ];
        filters
            .iter()
            .all(|f| f.filter(&mut state, pod, &hypo).is_success())
    }
}

impl PostFilterPlugin for DefaultPreemption {
    fn name(&self) -> &'static str {
        "DefaultPreemption"
    }

    fn post_filter(
        &self,
        _state: &mut CycleState,
        pod: &Pod,
        nodes: &[NodeInfo],
        _filter_failures: &FilterFailureMap,
    ) -> (Option<String>, Status) {
        // 1. Pods with priority < pod.priority on a node are candidates.
        // 2. For each node, evict the lowest-priority pods until the pod fits.
        // 3. Return the first node where the eviction set is non-empty.
        let candidate_priority = pod.spec.priority;
        let mut best: Option<(String, i64)> = None;

        for info in nodes {
            // Order victims by ascending priority so we evict the cheapest ones first.
            let mut victims: Vec<Pod> = info
                .pods()
                .iter()
                .filter(|p| p.spec.priority < candidate_priority)
                .cloned()
                .collect();
            victims.sort_by_key(|p| p.spec.priority);

            let mut chosen: Vec<Pod> = Vec::new();
            for v in victims {
                if Self::fits_after_eviction(pod, info, &chosen) {
                    break;
                }
                chosen.push(v);
            }

            if chosen.is_empty() {
                continue;
            }
            if !Self::fits_after_eviction(pod, info, &chosen) {
                continue;
            }
            // Lower cost = better candidate; cost = (#victims, max victim priority).
            let cost = chosen.len() as i64;
            let better = match &best {
                None => true,
                Some((_, prev)) => cost < *prev,
            };
            if better {
                best = Some((info.node().metadata.name.clone(), cost));
            }
        }

        match best {
            Some((name, _)) => (
                Some(name.clone()),
                Status {
                    code: crate::framework::Code::Success,
                    plugin: self.name().into(),
                    reasons: vec![format!("nominated node {name} after preemption")],
                },
            ),
            None => (
                None,
                Status::unschedulable(self.name(), "preemption found no candidate node"),
            ),
        }
    }
}

// Re-export MAX_NODE_SCORE so the `_ = MAX_NODE_SCORE` is not dead.
const _: i64 = MAX_NODE_SCORE;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Container, Node, Pod, Quantity, ResourceName};

    fn node(name: &str, cpu_cap: i64) -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = name.into();
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_cap));
        n.status
            .allocatable
            .insert(ResourceName::Memory, Quantity::bytes(8192));
        NodeInfo::new(n)
    }

    fn pod(name: &str, priority: i32, cpu_m: i64) -> Pod {
        let mut p = Pod::default();
        p.metadata.name = name.into();
        p.metadata.uid = name.into();
        p.spec.priority = priority;
        let mut c = Container::default();
        if cpu_m > 0 {
            c.resources
                .requests
                .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
        }
        p.spec.containers.push(c);
        p
    }

    #[test]
    fn preemption_picks_node_after_evicting_lower_priority() {
        let mut info = node("n1", 1000);
        let victim = pod("victim", 0, 800);
        info.add_pod(victim);
        let incoming = pod("important", 100, 500);
        let mut s = CycleState::new();
        let (nominee, status) = DefaultPreemption::new().post_filter(
            &mut s,
            &incoming,
            &[info],
            &FilterFailureMap::new(),
        );
        assert!(status.is_success(), "{:?}", status);
        assert_eq!(nominee.as_deref(), Some("n1"));
    }

    #[test]
    fn preemption_fails_when_no_lower_priority_victims() {
        let mut info = node("n1", 1000);
        let stayer = pod("stayer", 200, 800);
        info.add_pod(stayer);
        let incoming = pod("important", 100, 500);
        let mut s = CycleState::new();
        let (nominee, status) = DefaultPreemption::new().post_filter(
            &mut s,
            &incoming,
            &[info],
            &FilterFailureMap::new(),
        );
        assert!(nominee.is_none());
        assert!(!status.is_success());
    }

    #[test]
    fn preemption_skips_nodes_where_eviction_still_does_not_fit() {
        let info = node("n1", 100);
        let incoming = pod("important", 100, 9000);
        let mut s = CycleState::new();
        let (nominee, _) = DefaultPreemption::new().post_filter(
            &mut s,
            &incoming,
            &[info],
            &FilterFailureMap::new(),
        );
        assert!(nominee.is_none());
    }
}
