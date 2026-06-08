//! Quorum / membership for a small (1–3 node) cluster.
//!
//! cave-home is not a datacenter: a typical home is 1–3 nodes. "Quorum" here is
//! deliberately simple — the question the homeowner cares about is **"is my home
//! still working?"** We answer it at three levels ([`ClusterStatus`]):
//!
//! - **Operational** — there is a single healthy active primary. The home runs
//!   normally.
//! - **Degraded** — the home still works, but its safety margin is gone: the
//!   primary is only degraded, or there is no healthy backup left to take over,
//!   or a backup/GPU node is down. Worth a notification, not an alarm.
//! - **Down** — there is no healthy node able to act as the primary hub. The
//!   home is not automating; a human must step in.
//!
//! A formal consensus quorum (the leader-election / lease protocol) is Phase 1b
//! (consensus-bound, see the parity manifest). This module is the membership
//! summary that drives the homeowner's status tile, not a voting algorithm.

use crate::node::{NodeHealth, NodeRole};
use crate::topology::Cluster;

/// The overall operational status of the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClusterStatus {
    /// One healthy active primary, full redundancy where configured.
    Operational,
    /// Still running, but redundancy / safety margin is reduced.
    Degraded,
    /// No node can act as the primary hub — the home is not automating.
    Down,
}

/// Summarise the cluster status. Health must have been refreshed for the
/// relevant tick (use [`Cluster::refresh_all`] first).
#[must_use]
pub fn status(cluster: &Cluster) -> ClusterStatus {
    let primary = cluster.active_primary();

    // No single active primary, or it is unreachable: can a backup save us?
    let primary_alive = matches!(
        primary.map(crate::node::Node::health),
        Some(NodeHealth::Healthy | NodeHealth::Degraded)
    );

    if !primary_alive {
        // The primary is gone. If a healthy backup exists we are Degraded
        // (failover can recover us); otherwise Down.
        return if cluster.healthy_backup().is_some() {
            ClusterStatus::Degraded
        } else {
            ClusterStatus::Down
        };
    }

    // Primary is alive. Operational only if it is fully Healthy AND every
    // configured node is healthy (full redundancy). Any shortfall is Degraded.
    let primary_healthy =
        primary.map(crate::node::Node::health) == Some(NodeHealth::Healthy);

    let has_backup_role = cluster.nodes().iter().any(|n| n.role() == NodeRole::BackupHub);
    let backup_healthy = cluster.healthy_backup().is_some();

    let all_member_nodes_healthy = cluster
        .nodes()
        .iter()
        .all(|n| n.health() == NodeHealth::Healthy);

    if primary_healthy && all_member_nodes_healthy {
        ClusterStatus::Operational
    } else if primary_healthy && has_backup_role && !backup_healthy {
        // Primary fine but the backup we rely on for failover is not healthy:
        // we have lost our safety margin.
        ClusterStatus::Degraded
    } else if primary_healthy {
        // Primary fine, no backup configured (single-node home) but a non-
        // critical member (e.g. GPU node) is unhealthy -> Degraded.
        ClusterStatus::Degraded
    } else {
        // Primary only Degraded (late heartbeats) -> Degraded.
        ClusterStatus::Degraded
    }
}

impl ClusterStatus {
    /// Whether the home is automating at all (Operational or Degraded both
    /// keep the lights working; Down does not).
    #[must_use]
    pub const fn is_serving(self) -> bool {
        matches!(self, Self::Operational | Self::Degraded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn at(c: &mut Cluster, now: u64) -> ClusterStatus {
        c.refresh_all(now);
        status(c)
    }

    #[test]
    fn single_healthy_primary_is_operational() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary).with_radios());
        c.set_heartbeat("hub-1", 1000);
        assert_eq!(at(&mut c, 1000), ClusterStatus::Operational);
    }

    #[test]
    fn primary_plus_healthy_backup_is_operational() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary));
        c.add(Node::new("hub-2", "Backup hub", NodeRole::BackupHub));
        c.set_heartbeat("hub-1", 1000);
        c.set_heartbeat("hub-2", 1000);
        assert_eq!(at(&mut c, 1000), ClusterStatus::Operational);
    }

    #[test]
    fn primary_healthy_but_backup_down_is_degraded() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary));
        c.add(Node::new("hub-2", "Backup hub", NodeRole::BackupHub));
        c.set_heartbeat("hub-1", 1000);
        c.set_heartbeat("hub-2", 100); // backup stale -> lost safety margin
        assert_eq!(at(&mut c, 1000), ClusterStatus::Degraded);
    }

    #[test]
    fn primary_down_with_healthy_backup_is_degraded() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary));
        c.add(Node::new("hub-2", "Backup hub", NodeRole::BackupHub));
        c.set_heartbeat("hub-1", 100); // primary down
        c.set_heartbeat("hub-2", 1000); // backup ready to take over
        assert_eq!(at(&mut c, 1000), ClusterStatus::Degraded);
    }

    #[test]
    fn primary_down_no_backup_is_down() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary));
        c.set_heartbeat("hub-1", 100); // down
        assert_eq!(at(&mut c, 1000), ClusterStatus::Down);
    }

    #[test]
    fn degraded_primary_is_degraded_not_operational() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary));
        c.set_heartbeat("hub-1", 990); // age 10 -> degraded
        assert_eq!(at(&mut c, 1000), ClusterStatus::Degraded);
    }

    #[test]
    fn gpu_node_down_degrades_but_keeps_serving() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary));
        c.add(Node::new("gpu-1", "Camera box", NodeRole::MlGpu).with_gpu());
        c.set_heartbeat("hub-1", 1000);
        c.set_heartbeat("gpu-1", 100); // gpu down
        let s = at(&mut c, 1000);
        assert_eq!(s, ClusterStatus::Degraded);
        assert!(s.is_serving());
    }

    #[test]
    fn down_is_not_serving() {
        assert!(!ClusterStatus::Down.is_serving());
        assert!(ClusterStatus::Operational.is_serving());
        assert!(ClusterStatus::Degraded.is_serving());
    }

    #[test]
    fn empty_cluster_is_down() {
        let mut c = Cluster::new();
        assert_eq!(at(&mut c, 1000), ClusterStatus::Down);
    }
}
