//! The cluster topology — the set of nodes plus the invariants over them.
//!
//! A [`Cluster`] owns its nodes (keyed by id) and answers the structural
//! questions: is this a valid topology? who is the active primary? which nodes
//! could take over? The headline invariant (Charter §5) is **exactly one active
//! primary hub** — two active primaries is split-brain, zero primary-capable
//! nodes is an un-runnable home.

use crate::failover::{FailbackPolicy, FailoverPlan, FenceStatus, decide_failover};
use crate::health::HealthThresholds;
use crate::node::{Node, NodeHealth, NodeRole};

/// Why a topology is not valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyError {
    /// More than one node is currently the active [`NodeRole::Primary`].
    /// This is split-brain — the cluster must never present two primaries.
    TwoActivePrimaries { ids: Vec<String> },
    /// No node in the cluster can ever be the primary hub (all ML/GPU, or
    /// empty). The home has nowhere to run the broker / radios / automation.
    NoPrimaryCapableNode,
    /// A node id was added twice.
    DuplicateNodeId { id: String },
}

impl core::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TwoActivePrimaries { ids } => {
                write!(f, "two active primary hubs: {}", ids.join(", "))
            }
            Self::NoPrimaryCapableNode => {
                f.write_str("no node can act as the primary hub")
            }
            Self::DuplicateNodeId { id } => write!(f, "duplicate node id: {id}"),
        }
    }
}

impl std::error::Error for TopologyError {}

/// A cave-home cluster: a small set of nodes (typically 1–3) and the policies
/// that govern failover.
#[derive(Debug, Clone, Default)]
pub struct Cluster {
    nodes: Vec<Node>,
    thresholds: HealthThresholds,
    failback: FailbackPolicy,
}

impl Cluster {
    /// An empty cluster with default health thresholds and a manual failback
    /// policy (the safe default — a human confirms before the old primary
    /// reclaims the role).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the health thresholds used by [`Cluster::refresh_all`] and failover.
    #[must_use]
    pub const fn with_thresholds(mut self, thresholds: HealthThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// Set the failback policy applied when a former primary returns.
    #[must_use]
    pub const fn with_failback(mut self, failback: FailbackPolicy) -> Self {
        self.failback = failback;
        self
    }

    /// Add a node. If a node with the same id already exists it is **rejected**
    /// (returns `false`) rather than silently replaced, so a join can detect a
    /// collision.
    pub fn add(&mut self, node: Node) -> bool {
        if self.nodes.iter().any(|n| n.id() == node.id()) {
            return false;
        }
        self.nodes.push(node);
        true
    }

    /// All nodes, in insertion order.
    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Look up a node by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id() == id)
    }

    /// Record a heartbeat for a node. Returns `false` if the id is unknown.
    pub fn set_heartbeat(&mut self, id: &str, tick: u64) -> bool {
        self.nodes.iter_mut().find(|n| n.id() == id).is_some_and(|n| {
            n.set_heartbeat(tick);
            true
        })
    }

    /// Recompute every node's health against `now` and the cluster thresholds.
    pub fn refresh_all(&mut self, now: u64) {
        let t = self.thresholds;
        for n in &mut self.nodes {
            n.refresh_health(now, t);
        }
    }

    /// The ids of every node currently in the active-primary role.
    #[must_use]
    pub fn active_primaries(&self) -> Vec<&str> {
        self.nodes
            .iter()
            .filter(|n| n.role() == NodeRole::Primary)
            .map(Node::id)
            .collect()
    }

    /// The single active primary, if (and only if) there is exactly one.
    #[must_use]
    pub fn active_primary(&self) -> Option<&Node> {
        let mut primaries = self.nodes.iter().filter(|n| n.role() == NodeRole::Primary);
        let first = primaries.next()?;
        if primaries.next().is_some() {
            None // two+ primaries — not a single active primary.
        } else {
            Some(first)
        }
    }

    /// Validate the structural invariants.
    ///
    /// # Errors
    /// - [`TopologyError::DuplicateNodeId`] if two nodes share an id.
    /// - [`TopologyError::TwoActivePrimaries`] if more than one node is in the
    ///   active-primary role (split-brain).
    /// - [`TopologyError::NoPrimaryCapableNode`] if no node could ever be the
    ///   primary hub.
    pub fn validate(&self) -> Result<(), TopologyError> {
        // Duplicate ids.
        for (i, n) in self.nodes.iter().enumerate() {
            if self.nodes[..i].iter().any(|m| m.id() == n.id()) {
                return Err(TopologyError::DuplicateNodeId { id: n.id().to_owned() });
            }
        }
        // Exactly-one-active-primary: reject two+.
        let primaries = self.active_primaries();
        if primaries.len() > 1 {
            return Err(TopologyError::TwoActivePrimaries {
                ids: primaries.iter().map(|s| (*s).to_owned()).collect(),
            });
        }
        // At least one primary-capable node must exist.
        if !self.nodes.iter().any(|n| n.role().is_primary_capable()) {
            return Err(TopologyError::NoPrimaryCapableNode);
        }
        Ok(())
    }

    /// The best backup-hub candidate to promote: a [`NodeRole::BackupHub`] that
    /// is [`NodeHealth::Healthy`]. Among healthy backups the first in insertion
    /// order wins (deterministic).
    #[must_use]
    pub fn healthy_backup(&self) -> Option<&Node> {
        self.nodes
            .iter()
            .find(|n| n.role() == NodeRole::BackupHub && n.health() == NodeHealth::Healthy)
    }

    /// Decide whether to fail over, given the current tick and a fencing result.
    ///
    /// This refreshes health first (the decision must be made against current
    /// observations) and then delegates to [`crate::failover::decide_failover`].
    pub fn decide_failover(&mut self, now: u64, fence: FenceStatus) -> FailoverPlan {
        self.refresh_all(now);
        decide_failover(self, fence)
    }

    /// Apply a [`FailoverPlan::Promote`] to the topology: demote the named old
    /// primary (if present) to a backup and promote the named new node to the
    /// active primary. Returns `false` for any non-promote plan or an unknown
    /// node. After a successful apply the single-active-primary invariant holds.
    pub fn apply_failover(&mut self, plan: &FailoverPlan) -> bool {
        let FailoverPlan::Promote { node, demote } = plan else {
            return false;
        };
        if !self.nodes.iter().any(|n| n.id() == node) {
            return false;
        }
        for n in &mut self.nodes {
            if Some(n.id()) == demote.as_deref() {
                n.set_role(NodeRole::BackupHub);
            }
            if n.id() == node {
                n.set_role(NodeRole::Primary);
            }
        }
        true
    }

    #[must_use]
    pub const fn thresholds(&self) -> HealthThresholds {
        self.thresholds
    }

    #[must_use]
    pub const fn failback(&self) -> FailbackPolicy {
        self.failback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cluster_with(roles: &[(&str, NodeRole)]) -> Cluster {
        let mut c = Cluster::new();
        for (id, role) in roles {
            c.add(Node::new(id, id, *role));
        }
        c
    }

    #[test]
    fn single_primary_is_valid() {
        let c = cluster_with(&[("hub-1", NodeRole::Primary)]);
        assert!(c.validate().is_ok());
        assert_eq!(c.active_primary().map(Node::id), Some("hub-1"));
    }

    #[test]
    fn two_active_primaries_rejected() {
        let c = cluster_with(&[
            ("hub-1", NodeRole::Primary),
            ("hub-2", NodeRole::Primary),
        ]);
        match c.validate() {
            Err(TopologyError::TwoActivePrimaries { ids }) => assert_eq!(ids.len(), 2),
            other => panic!("expected TwoActivePrimaries, got {other:?}"),
        }
        // active_primary() must refuse to name one of two.
        assert_eq!(c.active_primary(), None);
    }

    #[test]
    fn no_primary_capable_node_rejected() {
        let c = cluster_with(&[("gpu-1", NodeRole::MlGpu)]);
        assert_eq!(c.validate(), Err(TopologyError::NoPrimaryCapableNode));
    }

    #[test]
    fn empty_cluster_rejected() {
        let c = Cluster::new();
        assert_eq!(c.validate(), Err(TopologyError::NoPrimaryCapableNode));
    }

    #[test]
    fn backup_only_is_primary_capable_and_valid() {
        // A lone backup hub is primary-capable, so the topology is valid even
        // before promotion (it is the bootstrap state of a recovered cluster).
        let c = cluster_with(&[("hub-2", NodeRole::BackupHub)]);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn duplicate_id_is_refused_on_add() {
        let mut c = Cluster::new();
        assert!(c.add(Node::new("hub-1", "A", NodeRole::Primary)));
        assert!(!c.add(Node::new("hub-1", "B", NodeRole::BackupHub)));
        assert_eq!(c.nodes().len(), 1);
    }

    #[test]
    fn healthy_backup_picks_first_healthy_backup() {
        let mut c = cluster_with(&[
            ("hub-1", NodeRole::Primary),
            ("hub-2", NodeRole::BackupHub),
            ("hub-3", NodeRole::BackupHub),
        ]);
        c.set_heartbeat("hub-2", 1000);
        c.set_heartbeat("hub-3", 1000);
        c.refresh_all(1000);
        assert_eq!(c.healthy_backup().map(Node::id), Some("hub-2"));
    }

    #[test]
    fn unhealthy_backup_is_not_a_candidate() {
        let mut c = cluster_with(&[
            ("hub-1", NodeRole::Primary),
            ("hub-2", NodeRole::BackupHub),
        ]);
        c.set_heartbeat("hub-2", 100); // stale
        c.refresh_all(1000);
        assert_eq!(c.healthy_backup(), None);
    }

    #[test]
    fn apply_failover_makes_backup_the_sole_primary() {
        let mut c = cluster_with(&[
            ("hub-1", NodeRole::Primary),
            ("hub-2", NodeRole::BackupHub),
        ]);
        let plan = FailoverPlan::Promote {
            node: "hub-2".to_owned(),
            demote: Some("hub-1".to_owned()),
        };
        assert!(c.apply_failover(&plan));
        assert_eq!(c.active_primaries(), vec!["hub-2"]);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn apply_failover_rejects_unknown_node() {
        let mut c = cluster_with(&[("hub-1", NodeRole::Primary)]);
        let plan = FailoverPlan::Promote { node: "ghost".to_owned(), demote: None };
        assert!(!c.apply_failover(&plan));
    }

    #[test]
    fn set_heartbeat_unknown_id_returns_false() {
        let mut c = cluster_with(&[("hub-1", NodeRole::Primary)]);
        assert!(!c.set_heartbeat("nope", 10));
        assert!(c.set_heartbeat("hub-1", 10));
    }
}
