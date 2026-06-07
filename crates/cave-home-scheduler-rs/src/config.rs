// SPDX-License-Identifier: Apache-2.0
//! Scheduler configuration — the Phase 2 slice of `KubeSchedulerConfiguration`.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/apis/config/types.go
//!         pkg/scheduler/schedule_one.go::numFeasibleNodesToFind
//!
//! Phase 2 ships a single hard-coded profile (`default-scheduler`); multi-
//! profile registries remain deferred (see `parity.manifest.toml`). What *is*
//! load-bearing here is `percentageOfNodesToScore` and the adaptive
//! `numFeasibleNodesToFind` heuristic, which bounds how many feasible nodes the
//! filter loop searches for on large clusters.

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
