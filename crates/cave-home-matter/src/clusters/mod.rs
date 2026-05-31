// SPDX-License-Identifier: Apache-2.0
//! Per-cluster client implementations.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/app/clusters/
//!
//! Phase 1 ships **commissioner-perspective clients** for the 5
//! essential lighting/HVAC/lock clusters plus Network Commissioning
//! (Thread credentials). Server-side (accessory) handlers are
//! `[[unmapped]] phase-1b`.

pub mod color_control;
pub mod door_lock;
pub mod level_control;
pub mod network_commissioning;
pub mod on_off;
pub mod thermostat;
pub mod window_covering;

use crate::error::Result;
use crate::fabric::NodeId;

/// Common shape for all cluster clients.
///
/// # Upstream: src/app/CommandSender.cpp::CommandSender (operationally)
pub trait ClusterClient {
    /// Cluster ID — used by the dispatcher.
    fn cluster_id(&self) -> u32;
    /// Trigger a no-op refresh of the client's cached attribute state.
    fn refresh(&self, node: NodeId) -> Result<()>;
}
