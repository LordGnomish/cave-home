// SPDX-License-Identifier: Apache-2.0
//! NetworkCommissioning cluster (0x0031) client.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/app/clusters/network-commissioning/network-commissioning.cpp
//!
//! Phase 1 ports the Thread provisioning subset: AddOrUpdateThreadNetwork
//! + ConnectNetwork. Wi-Fi provisioning lives behind the same trait and
//! lands in Phase 1b (cave-home assumes Wi-Fi is already up via the
//! homeowner's normal Wi-Fi setup; Thread is the credential-required
//! commissioning path).

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::clusters::ClusterClient;
use crate::error::{MatterError, Result};
use crate::fabric::NodeId;

/// Matter cluster id.
pub const CLUSTER_ID: u32 = 0x0031;

/// Thread Operational Dataset (TLV-encoded). Phase 1 carries it
/// verbatim across the wire.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadOperationalDataset {
    pub network_name: String,
    pub extended_pan_id: [u8; 8],
    pub master_key: [u8; 16],
    pub channel: u8,
    pub pan_id: u16,
}

impl ThreadOperationalDataset {
    /// 11..26 inclusive per IEEE 802.15.4 2450 MHz band.
    pub fn validate(&self) -> Result<()> {
        if !(11..=26).contains(&self.channel) {
            return Err(MatterError::InvalidArgument(format!(
                "Thread channel {} not in [11, 26]",
                self.channel
            )));
        }
        if self.network_name.is_empty() || self.network_name.len() > 16 {
            return Err(MatterError::InvalidArgument(
                "Thread network name must be 1..16 chars".into(),
            ));
        }
        if self.master_key == [0u8; 16] {
            return Err(MatterError::InvalidArgument(
                "Thread master key zero".into(),
            ));
        }
        Ok(())
    }
}

/// Per-node provisioning state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvisioningState {
    pub dataset: ThreadOperationalDataset,
    pub connected: bool,
}

/// NetworkCommissioning client.
#[derive(Debug, Default)]
pub struct NetworkCommissioningClient {
    state: Mutex<BTreeMap<NodeId, ProvisioningState>>,
}

impl NetworkCommissioningClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// AddOrUpdateThreadNetwork command.
    ///
    /// # Upstream: src/app/clusters/network-commissioning/network-commissioning.cpp::AddOrUpdateThreadNetwork
    pub fn add_thread_network(
        &self,
        node: NodeId,
        dataset: ThreadOperationalDataset,
    ) -> Result<()> {
        dataset.validate()?;
        self.state.lock().insert(
            node,
            ProvisioningState {
                dataset,
                connected: false,
            },
        );
        Ok(())
    }

    /// ConnectNetwork command — marks the previously-added dataset as
    /// joined.
    ///
    /// # Upstream: src/app/clusters/network-commissioning/network-commissioning.cpp::ConnectNetwork
    pub fn connect_network(&self, node: NodeId) -> Result<()> {
        let mut s = self.state.lock();
        let entry = s.get_mut(&node).ok_or_else(|| {
            MatterError::NotFound(format!("no Thread dataset added for {:?}", node))
        })?;
        entry.connected = true;
        Ok(())
    }

    /// Snapshot the per-node provisioning state.
    pub fn read_state(&self, node: NodeId) -> Result<ProvisioningState> {
        self.state
            .lock()
            .get(&node)
            .cloned()
            .ok_or_else(|| MatterError::NotFound(format!("node {:?}", node)))
    }
}

impl ClusterClient for NetworkCommissioningClient {
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

    fn dataset() -> ThreadOperationalDataset {
        ThreadOperationalDataset {
            network_name: "cave-home-Thread".into(),
            extended_pan_id: [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01, 0x02, 0x03],
            master_key: [0xAA; 16],
            channel: 15,
            pan_id: 0x1234,
        }
    }

    /// # Upstream: src/app/tests/network-commissioning/TestNetworkCommissioning.cpp::TestAddThreadNetwork
    #[test]
    fn add_thread_network_records_dataset() {
        let c = NetworkCommissioningClient::new();
        let n = NodeId(1);
        c.add_thread_network(n, dataset()).expect("add");
        let st = c.read_state(n).expect("read");
        assert_eq!(st.dataset.network_name, "cave-home-Thread");
        assert!(!st.connected);
    }

    #[test]
    fn connect_network_flips_flag() {
        let c = NetworkCommissioningClient::new();
        let n = NodeId(1);
        c.add_thread_network(n, dataset()).expect("add");
        c.connect_network(n).expect("connect");
        assert!(c.read_state(n).expect("read").connected);
    }

    #[test]
    fn connect_before_add_errors() {
        let c = NetworkCommissioningClient::new();
        assert!(c.connect_network(NodeId(1)).is_err());
    }

    #[test]
    fn dataset_validates_channel() {
        let mut d = dataset();
        d.channel = 10;
        assert!(d.validate().is_err());
        d.channel = 27;
        assert!(d.validate().is_err());
    }

    #[test]
    fn dataset_rejects_empty_name() {
        let mut d = dataset();
        d.network_name = String::new();
        assert!(d.validate().is_err());
    }

    #[test]
    fn dataset_rejects_zero_master_key() {
        let mut d = dataset();
        d.master_key = [0; 16];
        assert!(d.validate().is_err());
    }
}
