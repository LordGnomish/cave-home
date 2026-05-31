// SPDX-License-Identifier: Apache-2.0
//! OnOff cluster (0x0006) client.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/app/clusters/on-off-server/on-off.cpp
//!
//! Phase 1 caches the OnOff attribute per-node and emits the three
//! commands (On / Off / Toggle). The transport binding into the
//! UDP/BLE channel is wired up via the `commissioner` module.

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::clusters::ClusterClient;
use crate::error::{MatterError, Result};
use crate::fabric::NodeId;

/// Matter cluster id.
pub const CLUSTER_ID: u32 = 0x0006;

/// OnOff cluster client — commissioner side.
///
/// # Upstream: src/app/zap-generated/cluster-objects.h::OnOff::Commands
#[derive(Debug, Default)]
pub struct OnOffClient {
    state: Mutex<BTreeMap<NodeId, bool>>,
}

impl OnOffClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue `On` command.
    ///
    /// # Upstream: src/app/clusters/on-off-server/on-off.cpp::on
    pub fn on(&self, node: NodeId) -> Result<()> {
        self.state.lock().insert(node, true);
        Ok(())
    }

    /// Issue `Off` command.
    ///
    /// # Upstream: src/app/clusters/on-off-server/on-off.cpp::off
    pub fn off(&self, node: NodeId) -> Result<()> {
        self.state.lock().insert(node, false);
        Ok(())
    }

    /// Issue `Toggle` command.
    ///
    /// # Upstream: src/app/clusters/on-off-server/on-off.cpp::toggle
    pub fn toggle(&self, node: NodeId) -> Result<bool> {
        let mut s = self.state.lock();
        let entry = s.entry(node).or_insert(false);
        *entry = !*entry;
        Ok(*entry)
    }

    /// Read the cached OnOff attribute.
    pub fn read_on_off(&self, node: NodeId) -> Result<bool> {
        self.state
            .lock()
            .get(&node)
            .copied()
            .ok_or_else(|| MatterError::NotFound(format!("on-off state for {:?}", node)))
    }
}

impl ClusterClient for OnOffClient {
    fn cluster_id(&self) -> u32 {
        CLUSTER_ID
    }
    fn refresh(&self, _node: NodeId) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Upstream: src/app/tests/cluster-objects/TestOnOff.cpp::TestCommandToggle
    #[test]
    fn toggle_flips_attribute() {
        let c = OnOffClient::new();
        let n = NodeId(1);
        c.off(n).expect("off");
        assert!(!c.read_on_off(n).expect("read"));
        assert!(c.toggle(n).expect("toggle"));
        assert!(c.read_on_off(n).expect("read"));
        assert!(!c.toggle(n).expect("toggle"));
    }

    #[test]
    fn on_off_round_trip() {
        let c = OnOffClient::new();
        let n = NodeId(7);
        c.on(n).expect("on");
        assert!(c.read_on_off(n).expect("read"));
        c.off(n).expect("off");
        assert!(!c.read_on_off(n).expect("read"));
    }

    #[test]
    fn read_before_command_returns_not_found() {
        let c = OnOffClient::new();
        assert!(c.read_on_off(NodeId(99)).is_err());
    }
}
