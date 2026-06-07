// SPDX-License-Identifier: Apache-2.0
//! `NodeAffinity` (Score) — soft `preferredDuringScheduling` node affinity.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/nodeaffinity/node_affinity.go::Score
//!         + the shared `runtime.DefaultNormalizeScore`.
//!
//! The hard `requiredDuringScheduling` half is the `NodeAffinity` *Filter*
//! (see [`crate::plugins::NodeAffinityFilter`]); this is the complementary
//! Score half. A node earns the `weight` of every preferred term whose
//! `preference` `NodeSelectorTerm` it matches; the raw per-node sums are then
//! rescaled by [`ScorePlugin::normalize_score`] onto `[0, MAX_NODE_SCORE]`
//! relative to the highest-scoring node.

use crate::cache::NodeInfo;
use crate::framework::{CycleState, NodeScore, ScorePlugin, Status, MAX_NODE_SCORE};
use crate::types::Pod;

/// Upstream: `nodeaffinity.New` (the scoring half).
pub struct NodeAffinityScore;

impl ScorePlugin for NodeAffinityScore {
    fn name(&self) -> &'static str {
        "NodeAffinity"
    }

    /// Raw score = Σ weights of the pod's preferred terms whose `preference`
    /// matches this node's labels. A term matches when *all* of its
    /// match-expressions match (the same AND semantics as a hard
    /// `NodeSelectorTerm`). Pods with no preferred terms score `0`.
    fn score(&self, _state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> (i64, Status) {
        let labels = &node.node().metadata.labels;
        let mut sum = 0_i64;

        if let Some(term) = pod
            .spec
            .affinity
            .as_ref()
            .and_then(|a| a.node_affinity.as_ref())
        {
            for pref in &term.preferred_during_scheduling {
                let matches = pref
                    .preference
                    .match_expressions
                    .iter()
                    .all(|r| r.matches(labels));
                if matches {
                    sum = sum.saturating_add(i64::from(pref.weight));
                }
            }
        }

        (sum, Status::success())
    }

    /// Upstream `DefaultNormalizeScore(MaxNodeScore, reverse=false, scores)`:
    /// scale every node's raw score by `MAX_NODE_SCORE / max`, where `max` is
    /// the highest raw score. If every node scored `0` (or below), the scores
    /// are left untouched — there is nothing to differentiate.
    fn normalize_score(
        &self,
        _state: &mut CycleState,
        _pod: &Pod,
        scores: &mut [NodeScore],
    ) -> Status {
        let max = scores.iter().map(|s| s.score).max().unwrap_or(0);
        if max <= 0 {
            return Status::success();
        }
        for s in scores.iter_mut() {
            s.score = s.score.saturating_mul(MAX_NODE_SCORE) / max;
        }
        Status::success()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{
        Affinity, Node, NodeAffinity, NodeSelectorOperator, NodeSelectorRequirement,
        NodeSelectorTerm, Pod, PreferredSchedulingTerm,
    };

    fn node(name: &str, pairs: &[(&str, &str)]) -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = name.into();
        for (k, v) in pairs {
            n.metadata.labels.insert((*k).into(), (*v).into());
        }
        NodeInfo::new(n)
    }

    fn pref(key: &str, value: &str, weight: i32) -> PreferredSchedulingTerm {
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

    fn pod(terms: Vec<PreferredSchedulingTerm>) -> Pod {
        let mut p = Pod::default();
        p.spec.affinity = Some(Affinity {
            node_affinity: Some(NodeAffinity {
                required_during_scheduling: None,
                preferred_during_scheduling: terms,
            }),
        });
        p
    }

    #[test]
    fn matching_terms_sum_their_weights() {
        let n = node("n", &[("zone", "east"), ("disk", "ssd")]);
        let p = pod(vec![pref("zone", "east", 5), pref("disk", "ssd", 3)]);
        let mut s = CycleState::new();
        assert_eq!(NodeAffinityScore.score(&mut s, &p, &n).0, 8);
    }

    #[test]
    fn non_matching_term_contributes_nothing() {
        let n = node("n", &[("zone", "west")]);
        let p = pod(vec![pref("zone", "east", 9)]);
        let mut s = CycleState::new();
        assert_eq!(NodeAffinityScore.score(&mut s, &p, &n).0, 0);
    }

    #[test]
    fn normalize_scales_max_to_one_hundred() {
        let mut scores = vec![
            NodeScore {
                name: "a".into(),
                score: 4,
            },
            NodeScore {
                name: "b".into(),
                score: 1,
            },
        ];
        let mut s = CycleState::new();
        NodeAffinityScore.normalize_score(&mut s, &Pod::default(), &mut scores);
        assert_eq!(scores[0].score, 100);
        assert_eq!(scores[1].score, 25);
    }

    #[test]
    fn normalize_leaves_all_zero_untouched() {
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
        let mut s = CycleState::new();
        NodeAffinityScore.normalize_score(&mut s, &Pod::default(), &mut scores);
        assert!(scores.iter().all(|x| x.score == 0));
    }
}
