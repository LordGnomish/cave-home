// SPDX-License-Identifier: Apache-2.0
//! Single-pod scheduling cycle (`scheduleOne`).
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/schedule_one.go

use crate::cache::{NodeInfo, SchedulerCache};
use crate::framework::{CycleState, FilterFailureMap, NodeScore, PluginRegistry, Status};
use crate::types::Pod;

/// Upstream: `pkg/scheduler/schedule_one.go::ScheduleResult`.
#[derive(Debug, Clone)]
pub struct ScheduleResult {
    pub suggested_host: Option<String>,
    pub evaluated_nodes: usize,
    pub feasible_nodes: usize,
    pub nominated_node: Option<String>,
    pub filter_failures: FilterFailureMap,
    pub error: Option<Status>,
}

impl ScheduleResult {
    #[must_use]
    pub fn unschedulable(failures: FilterFailureMap, total: usize) -> Self {
        Self {
            suggested_host: None,
            evaluated_nodes: total,
            feasible_nodes: 0,
            nominated_node: None,
            filter_failures: failures,
            error: None,
        }
    }
}

/// Upstream: `Scheduler.scheduleOne` (the synchronous core).
pub fn schedule_one(
    pod: &Pod,
    cache: &SchedulerCache,
    registry: &PluginRegistry,
) -> ScheduleResult {
    let nodes = cache.snapshot();
    let total = nodes.len();
    let mut state = CycleState::new();

    // ---------- Filter ----------
    let mut feasible: Vec<NodeInfo> = Vec::with_capacity(nodes.len());
    let mut failures = FilterFailureMap::new();
    for node in &nodes {
        let mut accepted = true;
        for plugin in registry.filters() {
            let s = plugin.filter(&mut state, pod, node);
            if !s.is_success() {
                failures.insert(node.node().metadata.name.clone(), s);
                accepted = false;
                break;
            }
        }
        if accepted {
            feasible.push(node.clone());
        }
    }

    if feasible.is_empty() {
        // ---------- PostFilter (preemption) ----------
        for plugin in registry.post_filters() {
            let (nominee, status) = plugin.post_filter(&mut state, pod, &nodes, &failures);
            if status.is_success() && nominee.is_some() {
                return ScheduleResult {
                    suggested_host: None,
                    evaluated_nodes: total,
                    feasible_nodes: 0,
                    nominated_node: nominee,
                    filter_failures: failures,
                    error: None,
                };
            }
        }
        return ScheduleResult::unschedulable(failures, total);
    }

    // ---------- Score ----------
    // Upstream `prioritizeNodes`: for each plugin, score every feasible node,
    // run the plugin's NormalizeScore over the full result set, then add the
    // weighted result to each node's running total.
    let mut totals = vec![0_i64; feasible.len()];
    for plugin in registry.scores() {
        let mut node_scores: Vec<NodeScore> = Vec::with_capacity(feasible.len());
        for node in &feasible {
            let (score, status) = plugin.score(&mut state, pod, node);
            if !status.is_success() {
                return ScheduleResult {
                    suggested_host: None,
                    evaluated_nodes: total,
                    feasible_nodes: feasible.len(),
                    nominated_node: None,
                    filter_failures: failures,
                    error: Some(status),
                };
            }
            node_scores.push(NodeScore {
                name: node.node().metadata.name.clone(),
                score,
            });
        }

        let status = plugin.normalize_score(&mut state, pod, &mut node_scores);
        if !status.is_success() {
            return ScheduleResult {
                suggested_host: None,
                evaluated_nodes: total,
                feasible_nodes: feasible.len(),
                nominated_node: None,
                filter_failures: failures,
                error: Some(status),
            };
        }

        let weight = plugin.weight();
        for (acc, ns) in totals.iter_mut().zip(&node_scores) {
            *acc = acc.saturating_add(ns.score.saturating_mul(weight));
        }
    }

    let mut best_score = i64::MIN;
    let mut best_node: Option<String> = None;
    for (node, &total_score) in feasible.iter().zip(&totals) {
        if total_score > best_score {
            best_score = total_score;
            best_node = Some(node.node().metadata.name.clone());
        }
    }

    ScheduleResult {
        suggested_host: best_node,
        evaluated_nodes: total,
        feasible_nodes: feasible.len(),
        nominated_node: None,
        filter_failures: failures,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::SchedulerCache;
    use crate::plugins::default_registry;
    use crate::types::{Container, Node, ObjectMeta, Pod, Quantity, ResourceName};

    fn node(name: &str, cpu: i64, mem: i64) -> Node {
        let mut n = Node::default();
        n.metadata.name = name.into();
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu));
        n.status
            .allocatable
            .insert(ResourceName::Memory, Quantity::bytes(mem));
        n
    }

    fn pod(name: &str, cpu: i64, mem: i64) -> Pod {
        let mut p = Pod::default();
        p.metadata = ObjectMeta {
            namespace: "default".into(),
            name: name.into(),
            uid: name.into(),
            ..Default::default()
        };
        let mut c = Container::default();
        c.resources
            .requests
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu));
        c.resources
            .requests
            .insert(ResourceName::Memory, Quantity::bytes(mem));
        p.spec.containers.push(c);
        p
    }

    #[test]
    fn unschedulable_when_no_nodes() {
        let cache = SchedulerCache::new();
        let reg = default_registry();
        let result = schedule_one(&pod("p", 100, 256), &cache, &reg);
        assert!(result.suggested_host.is_none());
        assert_eq!(result.feasible_nodes, 0);
    }

    #[test]
    fn schedules_onto_only_fitting_node() {
        let cache = SchedulerCache::new();
        cache.add_node(node("small", 100, 256));
        cache.add_node(node("big", 2000, 4096));
        let reg = default_registry();
        let result = schedule_one(&pod("heavy", 1000, 1024), &cache, &reg);
        assert_eq!(result.suggested_host.as_deref(), Some("big"));
        assert_eq!(result.feasible_nodes, 1);
    }

    #[test]
    fn schedule_one_filters_then_scores() {
        let cache = SchedulerCache::new();
        cache.add_node(node("a", 1000, 1024));
        cache.add_node(node("b", 1000, 1024));
        let reg = default_registry();
        let result = schedule_one(&pod("light", 100, 100), &cache, &reg);
        assert!(result.suggested_host.is_some());
        assert_eq!(result.feasible_nodes, 2);
    }

    #[test]
    fn unschedulable_with_failure_reason_per_node() {
        let cache = SchedulerCache::new();
        cache.add_node(node("tiny", 100, 256));
        let reg = default_registry();
        let result = schedule_one(&pod("huge", 5000, 1), &cache, &reg);
        assert!(result.suggested_host.is_none());
        assert!(result.filter_failures.contains_key("tiny"));
    }
}
