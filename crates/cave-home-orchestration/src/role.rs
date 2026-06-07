//! Multi-node role assignment.
//!
//! ADR-005 (deployment topology) describes a cave-home deployment as a small
//! cluster: a primary hub (control-plane server), an optional backup hub
//! (another server, for HA), and optional worker/ML nodes (agents). This module
//! maps a homeowner's intent for each node onto the K3s role + cluster-start
//! decision, and validates the cluster as a whole.
//!
//! It is deliberately **independent of `cave-home-cluster`**: that crate owns
//! failover/placement; this one owns only the *orchestration* role mapping
//! (server vs agent, who runs `--cluster-init`). The two agree by convention —
//! server == control-plane node, agent == worker — but share no code.

use crate::component::Component;
use crate::config::ClusterStart;
use core::fmt;

/// What the operator intends a node to be, in homeowner terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeIntent {
    /// The first control-plane node — it initialises the cluster.
    PrimaryHub,
    /// An additional control-plane node for high availability — it joins.
    BackupHub,
    /// A worker (e.g. an ML/GPU off-load box) — it joins as an agent.
    Worker,
}

/// The orchestration role a node plays, derived from its intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestrationRole {
    /// Runs the control plane (apiserver/scheduler/controller-manager + kine).
    Server,
    /// Runs only the node-side agent components.
    Agent,
}

impl NodeIntent {
    /// The K3s role this intent maps to.
    #[must_use]
    pub const fn role(self) -> OrchestrationRole {
        match self {
            Self::PrimaryHub | Self::BackupHub => OrchestrationRole::Server,
            Self::Worker => OrchestrationRole::Agent,
        }
    }

    /// Whether this intent is a control-plane (server) node.
    #[must_use]
    pub const fn is_control_plane(self) -> bool {
        matches!(self.role(), OrchestrationRole::Server)
    }

    /// How this node should start: the primary hub initialises the cluster;
    /// everyone else joins the given server URL.
    #[must_use]
    pub fn cluster_start(self, server_url: &str) -> ClusterStart {
        match self {
            Self::PrimaryHub => ClusterStart::Init,
            Self::BackupHub | Self::Worker => ClusterStart::Join {
                server_url: server_url.to_owned(),
            },
        }
    }

    /// The components this node runs: a server runs the full set; an agent runs
    /// only the node-side components. Returned in declaration order.
    #[must_use]
    pub fn components(self) -> Vec<Component> {
        match self.role() {
            OrchestrationRole::Server => Component::ALL.to_vec(),
            OrchestrationRole::Agent => Component::ALL
                .iter()
                .copied()
                .filter(|c| !c.is_control_plane())
                .collect(),
        }
    }

    /// Prerequisites this node's components depend on but that are satisfied by
    /// *another* node (the remote control plane), not started locally.
    ///
    /// A server is self-contained, so this is empty. An agent's kubelet requires
    /// a reachable apiserver, which lives on the remote server — so the apiserver
    /// is an external prerequisite. Feed this to
    /// [`BringUpPlan::compute_with_external`](crate::bringup::BringUpPlan::compute_with_external).
    #[must_use]
    pub fn external_prerequisites(self) -> Vec<Component> {
        match self.role() {
            OrchestrationRole::Server => Vec::new(),
            OrchestrationRole::Agent => vec![Component::Apiserver],
        }
    }
}

/// Validate a whole-cluster set of intents.
///
/// Rules (ADR-005): exactly one [`NodeIntent::PrimaryHub`] (the cluster-init
/// node), and at least one node overall.
///
/// # Errors
/// - [`RoleError::NoNodes`] if the slice is empty.
/// - [`RoleError::NoPrimary`] if no node is the primary hub.
/// - [`RoleError::MultiplePrimaries`] if more than one node is the primary hub.
pub fn validate_cluster(intents: &[NodeIntent]) -> Result<(), RoleError> {
    if intents.is_empty() {
        return Err(RoleError::NoNodes);
    }
    let primaries = intents
        .iter()
        .filter(|i| matches!(i, NodeIntent::PrimaryHub))
        .count();
    match primaries {
        0 => Err(RoleError::NoPrimary),
        1 => Ok(()),
        n => Err(RoleError::MultiplePrimaries { count: n }),
    }
}

/// Why a cluster role assignment is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleError {
    /// No nodes were given.
    NoNodes,
    /// No node was designated the primary hub (cluster-init).
    NoPrimary,
    /// More than one node was designated the primary hub.
    MultiplePrimaries { count: usize },
}

impl fmt::Display for RoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoNodes => f.write_str("cluster has no nodes"),
            Self::NoPrimary => f.write_str("no primary hub (cluster-init node) designated"),
            Self::MultiplePrimaries { count } => {
                write!(f, "{count} primary hubs designated; exactly one is allowed")
            }
        }
    }
}

impl std::error::Error for RoleError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intents_map_to_expected_roles() {
        assert_eq!(NodeIntent::PrimaryHub.role(), OrchestrationRole::Server);
        assert_eq!(NodeIntent::BackupHub.role(), OrchestrationRole::Server);
        assert_eq!(NodeIntent::Worker.role(), OrchestrationRole::Agent);
        assert!(NodeIntent::PrimaryHub.is_control_plane());
        assert!(!NodeIntent::Worker.is_control_plane());
    }

    #[test]
    fn only_primary_hub_initialises_cluster() {
        assert_eq!(
            NodeIntent::PrimaryHub.cluster_start("https://hub-1:6443"),
            ClusterStart::Init
        );
        assert_eq!(
            NodeIntent::BackupHub.cluster_start("https://hub-1:6443"),
            ClusterStart::Join {
                server_url: "https://hub-1:6443".to_owned()
            }
        );
        assert_eq!(
            NodeIntent::Worker.cluster_start("https://hub-1:6443"),
            ClusterStart::Join {
                server_url: "https://hub-1:6443".to_owned()
            }
        );
    }

    #[test]
    fn server_runs_full_component_set() {
        let comps = NodeIntent::PrimaryHub.components();
        assert_eq!(comps.len(), Component::ALL.len());
        assert!(comps.contains(&Component::Apiserver));
        assert!(comps.contains(&Component::Kine));
    }

    #[test]
    fn agent_runs_only_node_side_components() {
        let comps = NodeIntent::Worker.components();
        assert!(comps.contains(&Component::Cni));
        assert!(comps.contains(&Component::Kubelet));
        assert!(comps.contains(&Component::KubeProxy));
        assert!(!comps.contains(&Component::Apiserver));
        assert!(!comps.contains(&Component::Kine));
        assert!(!comps.contains(&Component::Scheduler));
    }

    #[test]
    fn agent_component_set_is_orderable_with_external_apiserver() {
        // A worker's kubelet depends on the apiserver, which lives on the remote
        // control-plane node — so it is an *external* prerequisite, not a local
        // component. With that supplied, the agent set plans cleanly.
        let comps = NodeIntent::Worker.components();
        let ext = NodeIntent::Worker.external_prerequisites();
        assert_eq!(ext, vec![Component::Apiserver]);
        let plan = crate::bringup::BringUpPlan::compute_with_external(&comps, &ext);
        assert!(plan.is_ok(), "agent set must plan with external apiserver: {plan:?}");
    }

    #[test]
    fn server_has_no_external_prerequisites() {
        assert!(NodeIntent::PrimaryHub.external_prerequisites().is_empty());
        assert!(NodeIntent::BackupHub.external_prerequisites().is_empty());
    }

    #[test]
    fn single_primary_cluster_is_valid() {
        assert!(validate_cluster(&[NodeIntent::PrimaryHub]).is_ok());
        assert!(
            validate_cluster(&[
                NodeIntent::PrimaryHub,
                NodeIntent::BackupHub,
                NodeIntent::Worker,
            ])
            .is_ok()
        );
    }

    #[test]
    fn empty_cluster_rejected() {
        assert_eq!(validate_cluster(&[]), Err(RoleError::NoNodes));
    }

    #[test]
    fn no_primary_rejected() {
        assert_eq!(
            validate_cluster(&[NodeIntent::BackupHub, NodeIntent::Worker]),
            Err(RoleError::NoPrimary)
        );
    }

    #[test]
    fn multiple_primaries_rejected() {
        assert_eq!(
            validate_cluster(&[NodeIntent::PrimaryHub, NodeIntent::PrimaryHub]),
            Err(RoleError::MultiplePrimaries { count: 2 })
        );
    }

    #[test]
    fn role_error_displays_without_panicking() {
        for e in [
            RoleError::NoNodes,
            RoleError::NoPrimary,
            RoleError::MultiplePrimaries { count: 3 },
        ] {
            assert!(!format!("{e}").is_empty());
        }
    }
}
