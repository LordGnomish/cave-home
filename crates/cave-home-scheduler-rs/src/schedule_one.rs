// SPDX-License-Identifier: Apache-2.0
//! Single-pod scheduling cycle (`scheduleOne`).
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/schedule_one.go

use std::collections::BTreeSet;

use crate::cache::{NodeInfo, SchedulerCache};
use crate::framework::{
    Code, CycleState, FilterFailureMap, MAX_NODE_SCORE, MIN_NODE_SCORE, NodeScore, PluginRegistry,
    Status,
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

/// Upstream: `RunPreFilterPlugins` + the node-set restriction. Returns the
/// candidate node set on success, or a boxed terminal [`ScheduleResult`] if a
/// `PreFilter` plugin made the pod unschedulable / errored.
fn apply_pre_filters(
    pod: &Pod,
    all_nodes: Vec<NodeInfo>,
    registry: &PluginRegistry,
    state: &mut CycleState,
) -> std::result::Result<Vec<NodeInfo>, Box<ScheduleResult>> {
    let total = all_nodes.len();
    let mut allowed: Option<BTreeSet<String>> = None;
    for plugin in registry.pre_filters() {
        let (result, status) = plugin.pre_filter(state, pod);
        if !status.is_success() {
            let mut failures = FilterFailureMap::new();
            for node in &all_nodes {
                failures.insert(node.node().metadata.name.clone(), status.clone());
            }
            if status.code == Code::Error {
                return Err(Box::new(ScheduleResult {
                    suggested_host: None,
                    evaluated_nodes: total,
                    feasible_nodes: 0,
                    nominated_node: None,
                    filter_failures: failures,
                    error: Some(status),
                }));
            }
            return Err(Box::new(ScheduleResult::unschedulable(failures, total)));
        }
        if let Some(names) = result.and_then(|r| r.node_names) {
            allowed = Some(match allowed {
                Some(existing) => existing.intersection(&names).cloned().collect(),
                None => names,
            });
        }
    }
    Ok(match &allowed {
        Some(names) => all_nodes
            .into_iter()
            .filter(|n| names.contains(&n.node().metadata.name))
            .collect(),
        None => all_nodes,
    })
}

/// Upstream: `Scheduler.scheduleOne` (the synchronous core), searching every
/// feasible node.
#[must_use]
pub fn schedule_one(
    pod: &Pod,
    cache: &SchedulerCache,
    registry: &PluginRegistry,
) -> ScheduleResult {
    schedule_one_limited(pod, cache, registry, usize::MAX)
}

/// As [`schedule_one`], but stops the Filter loop once `feasible_limit` nodes
/// have been accepted. Upstream: `findNodesThatFitPod` honouring
/// `numFeasibleNodesToFind` to bound latency on large clusters.
#[must_use]
pub fn schedule_one_limited(
    pod: &Pod,
    cache: &SchedulerCache,
    registry: &PluginRegistry,
    feasible_limit: usize,
) -> ScheduleResult {
    let all_nodes = cache.snapshot();
    let total = all_nodes.len();
    let mut state = CycleState::new();

    // ---------- PreFilter ----------
    let nodes = match apply_pre_filters(pod, all_nodes, registry, &mut state) {
        Ok(nodes) => nodes,
        Err(result) => return *result,
    };

    // ---------- Filter ----------
    let mut feasible: Vec<NodeInfo> = Vec::with_capacity(nodes.len());
    let mut failures = FilterFailureMap::new();
    for node in &nodes {
        if feasible.len() >= feasible_limit {
            break;
        }
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

    // ---------- PreScore ----------
    // Precompute scoring state over the feasible set (upstream `RunPreScorePlugins`).
    for plugin in registry.pre_scores() {
        let status = plugin.pre_score(&mut state, pod, &feasible);
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
    }

    // ---------- Score ----------
    // Upstream `RunScorePlugins`: score is computed plugin-major so each plugin
    // can `NormalizeScore` over its whole `(node, raw-score)` set, after which
    // every score is range-checked and folded — weighted — into per-node totals.
    let mut totals: Vec<(String, i64)> = feasible
        .iter()
        .map(|n| (n.node().metadata.name.clone(), 0_i64))
        .collect();

    for plugin in registry.scores() {
        // Raw scores for this plugin across every feasible node.
        let mut scores: Vec<NodeScore> = Vec::with_capacity(feasible.len());
        for node in &feasible {
            let (score, status) = plugin.score(&mut state, pod, node);
            if !status.is_success() {
                return score_error(status, total, feasible.len(), failures);
            }
            scores.push(NodeScore::new(node.node().metadata.name.clone(), score));
        }

        // NormalizeScore (ScoreExtensions) — optional per-plugin rescale.
        let status = plugin.normalize_score(&mut state, pod, &mut scores);
        if !status.is_success() {
            return score_error(status, total, feasible.len(), failures);
        }

        // Range-check then weight-and-accumulate into the per-node totals.
        let weight = plugin.weight();
        for (slot, ns) in totals.iter_mut().zip(scores.iter()) {
            if ns.score < MIN_NODE_SCORE || ns.score > MAX_NODE_SCORE {
                let status = Status::error(
                    plugin.name(),
                    format!(
                        "score {} for node {} out of range [{MIN_NODE_SCORE}, {MAX_NODE_SCORE}]",
                        ns.score, ns.name
                    ),
                );
                return score_error(status, total, feasible.len(), failures);
            }
            slot.1 = slot.1.saturating_add(ns.score.saturating_mul(weight));
        }
    }

    // Highest total wins; ties resolve to the first (admission-order) node.
    let best_node = totals
        .into_iter()
        .max_by_key(|(_, s)| *s)
        .map(|(name, _)| name);

    ScheduleResult {
        suggested_host: best_node,
        evaluated_nodes: total,
        feasible_nodes: feasible.len(),
        nominated_node: None,
        filter_failures: failures,
        error: None,
    }
}

/// Build the terminal [`ScheduleResult`] for a Score-phase abort (a `Score` or
/// `NormalizeScore` plugin returned non-success, or a score fell out of range).
const fn score_error(
    status: Status,
    total: usize,
    feasible: usize,
    failures: FilterFailureMap,
) -> ScheduleResult {
    ScheduleResult {
        suggested_host: None,
        evaluated_nodes: total,
        feasible_nodes: feasible,
        nominated_node: None,
        filter_failures: failures,
        error: Some(status),
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
    fn feasible_node_search_stops_at_limit() {
        let cache = SchedulerCache::new();
        cache.add_node(node("n1", 1000, 1024));
        cache.add_node(node("n2", 1000, 1024));
        cache.add_node(node("n3", 1000, 1024));
        let reg = default_registry();
        // All three fit, but the limit caps the feasible search at one.
        let result = schedule_one_limited(&pod("p", 100, 100), &cache, &reg, 1);
        assert_eq!(result.feasible_nodes, 1);
        assert!(result.suggested_host.is_some());
    }

    /// Raw score 10 for every node except "n2" (which gets 90); NormalizeScore
    /// then inverts the set (`new = max - raw`). Without the normalize step the
    /// high-raw node "n2" would win; with it, the inversion makes "n1" win — so
    /// this proves `schedule_one` ranks on the *normalized* scores, not the raw.
    struct InvertingScore;
    impl ScorePlugin for InvertingScore {
        fn name(&self) -> &'static str {
            "InvertingScore"
        }
        fn score(&self, _: &mut CycleState, _: &Pod, node: &NodeInfo) -> (i64, Status) {
            let s = if node.node().metadata.name == "n2" {
                90
            } else {
                10
            };
            (s, Status::success())
        }
        fn normalize_score(
            &self,
            _: &mut CycleState,
            _: &Pod,
            scores: &mut [crate::framework::NodeScore],
        ) -> Status {
            let max = scores.iter().map(|s| s.score).max().unwrap_or(0);
            for s in scores.iter_mut() {
                s.score = max - s.score;
            }
            Status::success()
        }
    }

    #[test]
    fn normalize_score_inverts_ranking_before_selection() {
        let cache = SchedulerCache::new();
        cache.add_node(node("n1", 1000, 1024));
        cache.add_node(node("n2", 1000, 1024));
        let reg = crate::framework::PluginRegistry::builder()
            .with_score(Arc::new(InvertingScore))
            .build();
        let result = schedule_one(&pod("p", 100, 100), &cache, &reg);
        // Raw would pick n2 (90); normalized (n1=80, n2=0) picks n1.
        assert_eq!(result.suggested_host.as_deref(), Some("n1"));
    }

    /// Emits a score outside `[MIN_NODE_SCORE, MAX_NODE_SCORE]` and never
    /// normalizes it back in range — upstream aborts the cycle with an error.
    struct OutOfRangeScore;
    impl ScorePlugin for OutOfRangeScore {
        fn name(&self) -> &'static str {
            "OutOfRangeScore"
        }
        fn score(&self, _: &mut CycleState, _: &Pod, _: &NodeInfo) -> (i64, Status) {
            (999, Status::success())
        }
    }

    #[test]
    fn out_of_range_score_aborts_cycle() {
        let cache = SchedulerCache::new();
        cache.add_node(node("n1", 1000, 1024));
        let reg = crate::framework::PluginRegistry::builder()
            .with_score(Arc::new(OutOfRangeScore))
            .build();
        let result = schedule_one(&pod("p", 100, 100), &cache, &reg);
        assert!(result.suggested_host.is_none());
        assert!(result.error.is_some());
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
