// SPDX-License-Identifier: Apache-2.0
//! Scheduler configuration — the Phase 2 slice of `KubeSchedulerConfiguration`.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/apis/config/types.go
//!         `pkg/scheduler/schedule_one.go::numFeasibleNodesToFind`
//!
//! Phase 2 ships a single hard-coded profile (`default-scheduler`); multi-
//! profile registries remain deferred (see `parity.manifest.toml`). What *is*
//! load-bearing here is `percentageOfNodesToScore` and the adaptive
//! `numFeasibleNodesToFind` heuristic, which bounds how many feasible nodes the
//! filter loop searches for on large clusters.

/// Upstream: `pkg/scheduler/schedule_one.go::minFeasibleNodesToFind` — never
/// search for fewer than this many feasible nodes (small clusters are fully
/// searched anyway).
const MIN_FEASIBLE_NODES_TO_FIND: usize = 100;
/// Upstream: `basePercentageOfNodesToScore`.
const BASE_PERCENTAGE: u32 = 50;
/// Upstream: `minFeasibleNodesPercentageToFind`.
const MIN_PERCENTAGE: u32 = 5;

/// Upstream: `pkg/scheduler/apis/config/types.go::KubeSchedulerProfile`
/// (the Phase 2 slice). A single profile is supported.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Upstream: `KubeSchedulerProfile.SchedulerName`.
    pub profile_name: String,
    /// Upstream: `KubeSchedulerConfiguration.PercentageOfNodesToScore`.
    /// `None` selects the adaptive heuristic; `Some(p)` pins it to `p` percent.
    pub percentage_of_nodes_to_score: Option<u32>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            profile_name: "default-scheduler".to_string(),
            percentage_of_nodes_to_score: None,
        }
    }
}

impl SchedulerConfig {
    /// Pin `percentageOfNodesToScore` to an explicit value.
    #[must_use]
    pub const fn with_percentage(mut self, percentage: u32) -> Self {
        self.percentage_of_nodes_to_score = Some(percentage);
        self
    }

    /// Upstream: `pkg/scheduler/schedule_one.go::numFeasibleNodesToFind`.
    ///
    /// How many feasible nodes the filter loop should look for before stopping.
    /// Small clusters (and `percentage >= 100`) search everything; large
    /// clusters scale the search down adaptively to bound latency, never below
    /// the [`MIN_FEASIBLE_NODES_TO_FIND`] floor.
    #[must_use]
    pub fn num_feasible_nodes_to_find(&self, num_all_nodes: usize) -> usize {
        let percentage = self.percentage_of_nodes_to_score.unwrap_or(0);
        if num_all_nodes < MIN_FEASIBLE_NODES_TO_FIND || percentage >= 100 {
            return num_all_nodes;
        }

        // Adaptive percentage when unset/zero: shrink as the cluster grows.
        let adaptive = if percentage == 0 {
            let total = u32::try_from(num_all_nodes).unwrap_or(u32::MAX);
            BASE_PERCENTAGE
                .saturating_sub(total / 125)
                .max(MIN_PERCENTAGE)
        } else {
            percentage
        };

        let num = num_all_nodes.saturating_mul(adaptive as usize) / 100;
        if num < MIN_FEASIBLE_NODES_TO_FIND {
            MIN_FEASIBLE_NODES_TO_FIND
        } else {
            num
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_name_is_default_scheduler() {
        assert_eq!(SchedulerConfig::default().profile_name, "default-scheduler");
    }

    #[test]
    fn small_cluster_searches_all_nodes() {
        let cfg = SchedulerConfig::default();
        // Below the 100-node floor → consider every node.
        assert_eq!(cfg.num_feasible_nodes_to_find(10), 10);
        assert_eq!(cfg.num_feasible_nodes_to_find(99), 99);
    }

    #[test]
    fn explicit_full_percentage_searches_all_nodes() {
        let cfg = SchedulerConfig::default().with_percentage(100);
        assert_eq!(cfg.num_feasible_nodes_to_find(5000), 5000);
    }

    #[test]
    fn adaptive_percentage_scales_down_with_cluster_size() {
        let cfg = SchedulerConfig::default(); // adaptive (None)
        // 1000 nodes: 50 - 1000/125 = 42% → 420.
        assert_eq!(cfg.num_feasible_nodes_to_find(1000), 420);
    }

    #[test]
    fn adaptive_percentage_floors_at_five_percent() {
        let cfg = SchedulerConfig::default();
        // 10_000 nodes: 50 - 80 = -30 → clamped to 5% → 500.
        assert_eq!(cfg.num_feasible_nodes_to_find(10_000), 500);
    }

    #[test]
    fn explicit_percentage_is_floored_at_min_feasible() {
        // 200 nodes at 10% = 20, below the 100-node minimum → 100.
        let cfg = SchedulerConfig::default().with_percentage(10);
        assert_eq!(cfg.num_feasible_nodes_to_find(200), 100);
    }

    #[test]
    fn explicit_percentage_above_min_is_honored() {
        // 5000 nodes at 30% = 1500.
        let cfg = SchedulerConfig::default().with_percentage(30);
        assert_eq!(cfg.num_feasible_nodes_to_find(5000), 1500);
    }
}
