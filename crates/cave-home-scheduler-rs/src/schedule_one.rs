// SPDX-License-Identifier: Apache-2.0
//! Single-pod scheduling cycle (`scheduleOne`).
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/schedule_one.go

use crate::cache::{NodeInfo, SchedulerCache};
use crate::framework::{
    CycleState, FilterFailureMap, PluginRegistry, Status,
};
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
    let mut best_score = i64::MIN;
    let mut best_node: Option<String> = None;
    for node in &feasible {
        let mut total_score = 0_i64;
        for plugin in registry.scores() {
            let (score, status) = plugin.score(&mut state, pod, node);
            if !status.is_success() {
                // Score errors degrade to "no contribution" (upstream uses
                // an error code that aborts the scheduling cycle; here we
                // surface a single error code via the `error` field instead).
                return ScheduleResult {
                    suggested_host: None,
                    evaluated_nodes: total,
                    feasible_nodes: feasible.len(),
                    nominated_node: None,
                    filter_failures: failures,
                    error: Some(status),
                };
            }
            total_score = total_score.saturating_add(score.saturating_mul(plugin.weight()));
        }
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

    use crate::cache::NodeInfo;
    use crate::framework::{
        CycleState, PreFilterPlugin, PreFilterResult, PreScorePlugin, ScorePlugin, Status,
    };
    use std::collections::BTreeSet;
    use std::sync::Arc;

    /// PreFilter that restricts scheduling to an explicit node-name set.
    struct OnlyNodes(BTreeSet<String>);
    impl PreFilterPlugin for OnlyNodes {
        fn name(&self) -> &'static str {
            "OnlyNodes"
        }
        fn pre_filter(&self, _: &mut CycleState, _: &Pod) -> (Option<PreFilterResult>, Status) {
            (
                Some(PreFilterResult {
                    node_names: Some(self.0.clone()),
                }),
                Status::success(),
            )
        }
    }

    /// PreFilter that declares the pod globally unschedulable.
    struct RejectAll;
    impl PreFilterPlugin for RejectAll {
        fn name(&self) -> &'static str {
            "RejectAll"
        }
        fn pre_filter(&self, _: &mut CycleState, _: &Pod) -> (Option<PreFilterResult>, Status) {
            (None, Status::unschedulable(self.name(), "pod rejected pre-filter"))
        }
    }

    /// PreScore writes a per-node bonus into the cycle state...
    struct FavourPreScore;
    impl PreScorePlugin for FavourPreScore {
        fn name(&self) -> &'static str {
            "FavourPreScore"
        }
        fn pre_score(&self, state: &mut CycleState, _: &Pod, nodes: &[NodeInfo]) -> Status {
            // Record the favoured node = the lexicographically last node name.
            if let Some(last) = nodes.iter().map(|n| n.node().metadata.name.clone()).max() {
                state.write("favoured", last);
            }
            Status::success()
        }
    }

    /// ...which this Score reads to award the bonus.
    struct FavourScore;
    impl ScorePlugin for FavourScore {
        fn name(&self) -> &'static str {
            "FavourScore"
        }
        fn score(&self, state: &mut CycleState, _: &Pod, node: &NodeInfo) -> (i64, Status) {
            let favoured = state.read::<String>("favoured").cloned();
            let s = if favoured.as_deref() == Some(node.node().metadata.name.as_str()) {
                100
            } else {
                0
            };
            (s, Status::success())
        }
    }

    #[test]
    fn prefilter_node_subset_restricts_candidates() {
        let cache = SchedulerCache::new();
        cache.add_node(node("n1", 1000, 1024));
        cache.add_node(node("n2", 1000, 1024));
        let only: BTreeSet<String> = ["n2".to_string()].into_iter().collect();
        let reg = crate::framework::PluginRegistry::builder()
            .with_pre_filter(Arc::new(OnlyNodes(only)))
            .build();
        let result = schedule_one(&pod("p", 100, 100), &cache, &reg);
        assert_eq!(result.suggested_host.as_deref(), Some("n2"));
        assert_eq!(result.feasible_nodes, 1);
    }

    #[test]
    fn prefilter_unschedulable_short_circuits_before_filter() {
        let cache = SchedulerCache::new();
        cache.add_node(node("n1", 1000, 1024));
        let reg = crate::framework::PluginRegistry::builder()
            .with_pre_filter(Arc::new(RejectAll))
            .build();
        let result = schedule_one(&pod("p", 100, 100), &cache, &reg);
        assert!(result.suggested_host.is_none());
        assert_eq!(result.feasible_nodes, 0);
    }

    #[test]
    fn prescore_state_feeds_score_choice() {
        let cache = SchedulerCache::new();
        cache.add_node(node("n1", 1000, 1024));
        cache.add_node(node("n2", 1000, 1024));
        let reg = crate::framework::PluginRegistry::builder()
            .with_pre_score(Arc::new(FavourPreScore))
            .with_score(Arc::new(FavourScore))
            .build();
        let result = schedule_one(&pod("p", 100, 100), &cache, &reg);
        // FavourPreScore favours the last node name ("n2"); FavourScore awards it.
        assert_eq!(result.suggested_host.as_deref(), Some("n2"));
    }
}
