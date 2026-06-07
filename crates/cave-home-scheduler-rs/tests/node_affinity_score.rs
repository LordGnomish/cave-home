// SPDX-License-Identifier: Apache-2.0
//! Failing tests (RED) for the NodeAffinity *preferred* scoring plugin and the
//! framework `NormalizeScore` extension point it relies on.
//!
//! Behavioural reference: kubernetes/kubernetes
//! `pkg/scheduler/framework/plugins/nodeaffinity/node_affinity.go::Score` +
//! `ScoreExtensions.NormalizeScore`, and the framework's
//! `pkg/scheduler/framework/runtime/framework.go::DefaultNormalizeScore`.
//!
//! Contract under test:
//!   * `Score(pod, node)` = sum of the weights of the pod's
//!     `preferredDuringSchedulingIgnoredDuringExecution` terms whose
//!     `preference` `NodeSelectorTerm` matches the node's labels (raw, ≥ 0);
//!   * `NormalizeScore` rescales a plugin's raw per-node scores onto
//!     `[0, MaxNodeScore]` proportionally to the max raw score (all-zero stays
//!     zero), preserving slice order and length;
//!   * wired through `schedule_one`, a pod that prefers a node wins the tie.

use cave_home_scheduler_rs::cache::{NodeInfo, SchedulerCache};
use cave_home_scheduler_rs::framework::{CycleState, NodeScore, ScorePlugin, MAX_NODE_SCORE};
use cave_home_scheduler_rs::plugins::default_registry;
use cave_home_scheduler_rs::plugins::node_affinity_score::NodeAffinityScore;
use cave_home_scheduler_rs::schedule_one::schedule_one;
use cave_home_scheduler_rs::types::{
    Affinity, Container, Node, NodeAffinity, NodeSelectorOperator, NodeSelectorRequirement,
    NodeSelectorTerm, Pod, PreferredSchedulingTerm, Quantity, ResourceName,
};

fn prefers(key: &str, value: &str, weight: i32) -> PreferredSchedulingTerm {
    PreferredSchedulingTerm {
        weight,
        preference: NodeSelectorTerm {
            match_expressions: vec![NodeSelectorRequirement {
                key: key.into(),
                operator: Some(NodeSelectorOperator::In),
                values: vec![value.into()],
            }],
        },
    }
}

fn pod_preferring(terms: Vec<PreferredSchedulingTerm>) -> Pod {
    let mut p = Pod::default();
    p.spec.affinity = Some(Affinity {
        node_affinity: Some(NodeAffinity {
            required_during_scheduling: None,
            preferred_during_scheduling: terms,
        }),
    });
    p
}

fn node_labeled(name: &str, pairs: &[(&str, &str)]) -> NodeInfo {
    let mut n = Node::default();
    n.metadata.name = name.into();
    for (k, v) in pairs {
        n.metadata.labels.insert((*k).into(), (*v).into());
    }
    NodeInfo::new(n)
}

#[test]
fn raw_score_sums_matching_term_weights() {
    let node = node_labeled("n", &[("zone", "us-east"), ("disk", "ssd")]);
    let pod = pod_preferring(vec![
        prefers("zone", "us-east", 5),
        prefers("disk", "ssd", 3),
        prefers("zone", "us-west", 10), // no match
    ]);
    let mut s = CycleState::new();
    let (raw, status) = NodeAffinityScore.score(&mut s, &pod, &node);
    assert!(status.is_success());
    assert_eq!(raw, 8);
}

#[test]
fn raw_score_is_zero_without_preferred_terms() {
    let node = node_labeled("n", &[("zone", "us-east")]);
    let pod = Pod::default();
    let mut s = CycleState::new();
    let (raw, status) = NodeAffinityScore.score(&mut s, &pod, &node);
    assert!(status.is_success());
    assert_eq!(raw, 0);
}

#[test]
fn normalize_rescales_to_max_node_score() {
    let pod = Pod::default();
    let mut s = CycleState::new();
    let mut scores = vec![
        NodeScore {
            name: "a".into(),
            score: 8,
        },
        NodeScore {
            name: "b".into(),
            score: 0,
        },
    ];
    let status = NodeAffinityScore.normalize_score(&mut s, &pod, &mut scores);
    assert!(status.is_success());
    assert_eq!(scores[0].score, MAX_NODE_SCORE); // 8 -> 100
    assert_eq!(scores[1].score, 0);
}

#[test]
fn normalize_is_proportional() {
    let pod = Pod::default();
    let mut s = CycleState::new();
    let mut scores = vec![
        NodeScore {
            name: "a".into(),
            score: 2,
        },
        NodeScore {
            name: "b".into(),
            score: 4,
        },
    ];
    NodeAffinityScore.normalize_score(&mut s, &pod, &mut scores);
    assert_eq!(scores[0].score, 50); // 2/4 * 100
    assert_eq!(scores[1].score, 100);
}

#[test]
fn normalize_all_zero_stays_zero() {
    let pod = Pod::default();
    let mut s = CycleState::new();
    let mut scores = vec![
        NodeScore {
            name: "a".into(),
            score: 0,
        },
        NodeScore {
            name: "b".into(),
            score: 0,
        },
    ];
    NodeAffinityScore.normalize_score(&mut s, &pod, &mut scores);
    assert_eq!(scores[0].score, 0);
    assert_eq!(scores[1].score, 0);
}

// ---- full pipeline through schedule_one ------------------------------------

fn node_with_caps(name: &str, labels: &[(&str, &str)], cpu: i64, mem: i64) -> Node {
    let mut n = Node::default();
    n.metadata.name = name.into();
    for (k, v) in labels {
        n.metadata.labels.insert((*k).into(), (*v).into());
    }
    n.status
        .allocatable
        .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu));
    n.status
        .allocatable
        .insert(ResourceName::Memory, Quantity::bytes(mem));
    n
}

fn small_pod_preferring(terms: Vec<PreferredSchedulingTerm>) -> Pod {
    let mut p = pod_preferring(terms);
    p.metadata.name = "p".into();
    p.metadata.namespace = "default".into();
    let mut c = Container::default();
    c.resources
        .requests
        .insert(ResourceName::Cpu, Quantity::milli_cpu(100));
    c.resources
        .requests
        .insert(ResourceName::Memory, Quantity::bytes(128));
    p.spec.containers.push(c);
    p
}

#[test]
fn preferred_affinity_breaks_the_tie_between_equal_nodes() {
    let cache = SchedulerCache::new();
    // Identical capacity; only the label differs.
    cache.add_node(node_with_caps("east", &[("zone", "us-east")], 2000, 4096));
    cache.add_node(node_with_caps("west", &[("zone", "us-west")], 2000, 4096));
    let reg = default_registry();
    let pod = small_pod_preferring(vec![prefers("zone", "us-east", 100)]);
    let result = schedule_one(&pod, &cache, &reg);
    assert_eq!(result.suggested_host.as_deref(), Some("east"));
    assert_eq!(result.feasible_nodes, 2);
}

#[test]
fn pod_without_affinity_still_schedules() {
    let cache = SchedulerCache::new();
    cache.add_node(node_with_caps("only", &[], 2000, 4096));
    let reg = default_registry();
    let pod = small_pod_preferring(vec![]);
    let result = schedule_one(&pod, &cache, &reg);
    assert_eq!(result.suggested_host.as_deref(), Some("only"));
}
