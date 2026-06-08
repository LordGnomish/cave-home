//! Workload placement hints — which node should run a given workload.
//!
//! cave-home runs a handful of workload *classes*, each with a natural home
//! (Charter §5):
//!
//! - **Radios** (Zigbee / Matter / Z-Wave) must run where the radio hardware is
//!   physically attached — the primary hub with radios.
//! - **Camera inference** (Frigate object detection) wants the accelerator — the
//!   ML/GPU node, falling back to the active primary if there is no GPU node.
//! - **Automation engine** (the HA-core port) runs on the active primary — it is
//!   the behavioural heart and follows the primary on failover.
//! - **Broker / Portal** run on the active primary alongside automation.
//!
//! This module is a pure mapping from a [`Workload`] to a target node, with a
//! documented fallback when the ideal node is absent.

use crate::node::{Node, NodeHealth, NodeRole};
use crate::topology::Cluster;

/// A class of workload the cluster schedules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Workload {
    /// Zigbee / Matter / Z-Wave radio stacks — bound to the radio hardware.
    Radios,
    /// Camera object-detection inference — wants a GPU/accelerator.
    CameraInference,
    /// The automation engine (HA-core port) — follows the active primary.
    AutomationEngine,
    /// The MQTT broker — co-located with the active primary.
    Broker,
    /// The Portal web UI — co-located with the active primary.
    Portal,
}

/// Why a workload could not be placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementError {
    /// No node satisfies the workload's hardware/role requirement and no
    /// fallback is viable (e.g. radios requested but no node has the radios, or
    /// there is no active primary to host primary-bound workloads).
    NoEligibleNode,
}

impl core::fmt::Display for PlacementError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoEligibleNode => f.write_str("no node can run this workload"),
        }
    }
}

impl std::error::Error for PlacementError {}

/// A node is eligible to *host* a workload only if it is not draining and not
/// unreachable. Degraded is allowed (better a slow host than no host).
fn hostable(node: &Node) -> bool {
    !node.is_draining() && node.health() != NodeHealth::Unreachable
}

/// Decide which node should run `workload`, returning the chosen node id.
///
/// # Errors
/// Returns [`PlacementError::NoEligibleNode`] when no node can host it.
pub fn place(cluster: &Cluster, workload: Workload) -> Result<String, PlacementError> {
    match workload {
        Workload::Radios => {
            // Radios bind to the radio hardware. Prefer the active primary if it
            // has radios; otherwise any hostable node that has them.
            if let Some(p) = cluster.active_primary() {
                if p.capabilities().has_radios && hostable(p) {
                    return Ok(p.id().to_owned());
                }
            }
            cluster
                .nodes()
                .iter()
                .find(|n| n.capabilities().has_radios && hostable(n))
                .map(|n| n.id().to_owned())
                .ok_or(PlacementError::NoEligibleNode)
        }
        Workload::CameraInference => {
            // Prefer a hostable GPU node; fall back to the active primary.
            if let Some(gpu) = cluster.nodes().iter().find(|n| {
                n.role() == NodeRole::MlGpu && n.capabilities().has_gpu && hostable(n)
            }) {
                return Ok(gpu.id().to_owned());
            }
            // Fallback: any hostable node with a GPU, then the active primary.
            if let Some(any_gpu) =
                cluster.nodes().iter().find(|n| n.capabilities().has_gpu && hostable(n))
            {
                return Ok(any_gpu.id().to_owned());
            }
            primary_host(cluster)
        }
        // The behavioural core and its co-located services live on the active
        // primary.
        Workload::AutomationEngine | Workload::Broker | Workload::Portal => {
            primary_host(cluster)
        }
    }
}

/// Resolve the active-primary host for a primary-bound workload.
fn primary_host(cluster: &Cluster) -> Result<String, PlacementError> {
    cluster
        .active_primary()
        .filter(|p| hostable(p))
        .map(|p| p.id().to_owned())
        .ok_or(PlacementError::NoEligibleNode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::HealthThresholds;

    fn full_cluster() -> Cluster {
        let mut c = Cluster::new().with_thresholds(HealthThresholds::new(5, 15));
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
    fn radios_go_to_primary_with_radios() {
        let c = full_cluster();
        assert_eq!(place(&c, Workload::Radios), Ok("hub-1".to_owned()));
    }

    #[test]
    fn camera_inference_goes_to_gpu_node() {
        let c = full_cluster();
        assert_eq!(place(&c, Workload::CameraInference), Ok("gpu-1".to_owned()));
    }

    #[test]
    fn automation_engine_goes_to_active_primary() {
        let c = full_cluster();
        assert_eq!(place(&c, Workload::AutomationEngine), Ok("hub-1".to_owned()));
        assert_eq!(place(&c, Workload::Broker), Ok("hub-1".to_owned()));
        assert_eq!(place(&c, Workload::Portal), Ok("hub-1".to_owned()));
    }

    #[test]
    fn camera_inference_falls_back_to_primary_without_gpu_node() {
        // Single-node home: no GPU node, inference falls back to the primary.
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary).with_radios());
        c.set_heartbeat("hub-1", 1000);
        c.refresh_all(1000);
        assert_eq!(place(&c, Workload::CameraInference), Ok("hub-1".to_owned()));
    }

    #[test]
    fn radios_have_no_fallback_when_no_node_has_them() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary)); // no radios
        c.set_heartbeat("hub-1", 1000);
        c.refresh_all(1000);
        assert_eq!(place(&c, Workload::Radios), Err(PlacementError::NoEligibleNode));
    }

    #[test]
    fn draining_primary_cannot_host_primary_workloads() {
        // A draining primary (and no other active primary) leaves the automation
        // engine nowhere valid -> error, which prompts a failover first.
        let mut c = Cluster::new();
        let mut p = Node::new("hub-1", "Main hub", NodeRole::Primary).with_radios();
        p.set_heartbeat(1000);
        p.refresh_health(1000, HealthThresholds::default());
        p.set_draining(true);
        c.add(p);
        assert_eq!(
            place(&c, Workload::AutomationEngine),
            Err(PlacementError::NoEligibleNode)
        );
    }

    #[test]
    fn unreachable_primary_cannot_host() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary).with_radios());
        c.set_heartbeat("hub-1", 100); // stale
        c.refresh_all(1000);
        assert_eq!(
            place(&c, Workload::AutomationEngine),
            Err(PlacementError::NoEligibleNode)
        );
    }

    #[test]
    fn radios_fall_back_to_backup_with_radios_when_primary_lacks_them() {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary)); // no radios
        c.add(Node::new("hub-2", "Backup hub", NodeRole::BackupHub).with_radios());
        c.set_heartbeat("hub-1", 1000);
        c.set_heartbeat("hub-2", 1000);
        c.refresh_all(1000);
        assert_eq!(place(&c, Workload::Radios), Ok("hub-2".to_owned()));
    }
}
