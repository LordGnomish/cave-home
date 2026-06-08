//! Rolling-update / drain model — taking a node down safely.
//!
//! Updating cave-home means taking nodes down one at a time without breaking the
//! home (Charter §5: "keeps the home automating during firmware updates"). Two
//! questions:
//!
//! 1. **Is it safe to take this node down right now?** ([`DrainPlan::can_drain`])
//!    A node is safe to drain when the home can keep running without it: it is
//!    not the *sole* primary-capable node, and if it is the active primary, a
//!    healthy backup exists to take over first.
//! 2. **In what order do we update the whole cluster?** ([`DrainPlan::update_order`])
//!    Update the dispensable nodes first (GPU node, idle backup), and the active
//!    primary last — so the home runs on the most-tested-still-up node for as
//!    long as possible, and the primary moves only once a fresh backup is ready.
//!
//! The actual package swap / reboot is out of scope (it is the CLI / OS-image
//! job, ADR-005); this module decides *whether* and *in what order*.

use crate::node::{Node, NodeHealth, NodeRole};
use crate::topology::Cluster;

/// Why a node cannot be drained right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrainError {
    /// The node id is not in the cluster.
    UnknownNode { id: String },
    /// Draining this node would leave the home with no primary hub: it is the
    /// only primary-capable node, or it is the active primary and no healthy
    /// backup can take over.
    WouldStrandHome { id: String },
}

impl core::fmt::Display for DrainError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownNode { id } => write!(f, "unknown node: {id}"),
            Self::WouldStrandHome { id } => {
                write!(f, "draining {id} would leave the home with no working hub")
            }
        }
    }
}

impl std::error::Error for DrainError {}

/// Drain / rolling-update planner over a cluster snapshot.
#[derive(Debug, Clone, Copy)]
pub struct DrainPlan<'a> {
    cluster: &'a Cluster,
}

impl<'a> DrainPlan<'a> {
    #[must_use]
    pub const fn new(cluster: &'a Cluster) -> Self {
        Self { cluster }
    }

    /// Whether `id` can be drained without stranding the home.
    ///
    /// # Errors
    /// - [`DrainError::UnknownNode`] if the id is not present.
    /// - [`DrainError::WouldStrandHome`] if draining it would leave no working
    ///   primary hub.
    pub fn can_drain(&self, id: &str) -> Result<(), DrainError> {
        let Some(node) = self.cluster.get(id) else {
            return Err(DrainError::UnknownNode { id: id.to_owned() });
        };

        // A non-primary-capable node (ML/GPU) is always safe to drain — the home
        // keeps running without it (camera inference just pauses).
        if !node.role().is_primary_capable() {
            return Ok(());
        }

        // It is primary-capable. Count the OTHER primary-capable nodes that are
        // healthy enough to carry the home.
        let other_healthy_primary_capable = self
            .cluster
            .nodes()
            .iter()
            .filter(|n| {
                n.id() != id
                    && n.role().is_primary_capable()
                    && !n.is_draining()
                    && n.health() == NodeHealth::Healthy
            })
            .count();

        if node.role() == NodeRole::Primary {
            // Draining the active primary requires a healthy backup to fail over
            // to first.
            if other_healthy_primary_capable == 0 {
                return Err(DrainError::WouldStrandHome { id: id.to_owned() });
            }
            Ok(())
        } else {
            // Draining a standby backup is fine as long as the primary (or
            // another backup) is still up to keep serving / be failed over to.
            // If this backup is the ONLY healthy primary-capable node (primary
            // already down), draining it strands the home.
            let primary_serving = matches!(
                self.cluster.active_primary().map(Node::health),
                Some(NodeHealth::Healthy | NodeHealth::Degraded)
            );
            if primary_serving || other_healthy_primary_capable > 0 {
                Ok(())
            } else {
                Err(DrainError::WouldStrandHome { id: id.to_owned() })
            }
        }
    }

    /// The order to update the whole cluster: least-critical first, active
    /// primary last. Within a tier, insertion order is preserved (deterministic).
    ///
    /// Tier order: ML/GPU nodes, then backup hubs, then the active primary.
    #[must_use]
    pub fn update_order(&self) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        let push_role = |order: &mut Vec<String>, role: NodeRole| {
            for n in self.cluster.nodes() {
                if n.role() == role {
                    order.push(n.id().to_owned());
                }
            }
        };
        push_role(&mut order, NodeRole::MlGpu);
        push_role(&mut order, NodeRole::BackupHub);
        push_role(&mut order, NodeRole::Primary);
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cluster() -> Cluster {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary).with_radios());
        c.add(Node::new("hub-2", "Backup hub", NodeRole::BackupHub).with_radios());
        c.add(Node::new("gpu-1", "Camera box", NodeRole::MlGpu).with_gpu());
        c.set_heartbeat("hub-1", 1000);
        c.set_heartbeat("hub-2", 1000);
        c.set_heartbeat("gpu-1", 1000);
        c.refresh_all(1000);
        c
    }

    #[test]
    fn gpu_node_always_drainable() {
        let c = cluster();
        assert_eq!(DrainPlan::new(&c).can_drain("gpu-1"), Ok(()));
    }

    #[test]
    fn primary_drainable_with_healthy_backup() {
        let c = cluster();
        assert_eq!(DrainPlan::new(&c).can_drain("hub-1"), Ok(()));
    }

    #[test]
    fn sole_primary_not_drainable() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary).with_radios());
        c.set_heartbeat("hub-1", 1000);
        c.refresh_all(1000);
        assert_eq!(
            DrainPlan::new(&c).can_drain("hub-1"),
            Err(DrainError::WouldStrandHome { id: "hub-1".to_owned() })
        );
    }

    #[test]
    fn primary_not_drainable_when_backup_unhealthy() {
        let mut c = cluster();
        c.set_heartbeat("hub-2", 100); // backup down
        c.refresh_all(1000);
        assert_eq!(
            DrainPlan::new(&c).can_drain("hub-1"),
            Err(DrainError::WouldStrandHome { id: "hub-1".to_owned() })
        );
    }

    #[test]
    fn backup_drainable_while_primary_serving() {
        let c = cluster();
        assert_eq!(DrainPlan::new(&c).can_drain("hub-2"), Ok(()));
    }

    #[test]
    fn lone_backup_not_drainable_when_primary_down() {
        // Primary already down; the backup is the only thing keeping a path to a
        // working hub — draining it strands the home.
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary));
        c.add(Node::new("hub-2", "Backup hub", NodeRole::BackupHub));
        c.set_heartbeat("hub-1", 100); // down
        c.set_heartbeat("hub-2", 1000);
        c.refresh_all(1000);
        assert_eq!(
            DrainPlan::new(&c).can_drain("hub-2"),
            Err(DrainError::WouldStrandHome { id: "hub-2".to_owned() })
        );
    }

    #[test]
    fn unknown_node_drain_errors() {
        let c = cluster();
        assert_eq!(
            DrainPlan::new(&c).can_drain("ghost"),
            Err(DrainError::UnknownNode { id: "ghost".to_owned() })
        );
    }

    #[test]
    fn update_order_is_least_critical_first_primary_last() {
        let c = cluster();
        assert_eq!(
            DrainPlan::new(&c).update_order(),
            vec!["gpu-1".to_owned(), "hub-2".to_owned(), "hub-1".to_owned()]
        );
    }

    #[test]
    fn update_order_single_node() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary));
        assert_eq!(DrainPlan::new(&c).update_order(), vec!["hub-1".to_owned()]);
    }
}
