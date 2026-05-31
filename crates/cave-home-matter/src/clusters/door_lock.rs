// SPDX-License-Identifier: Apache-2.0
//! DoorLock cluster (0x0101) client.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/app/clusters/door-lock-server/door-lock-server.cpp

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::clusters::ClusterClient;
use crate::error::{MatterError, Result};
use crate::fabric::NodeId;

/// Matter cluster id.
pub const CLUSTER_ID: u32 = 0x0101;

/// Door lock state.
///
/// # Upstream: src/app/clusters/door-lock-server/door-lock-server.h::DlLockState
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DoorLockState {
    NotFullyLocked,
    Locked,
    Unlocked,
}

/// DoorLock client.
#[derive(Debug, Default)]
pub struct DoorLockClient {
    state: Mutex<BTreeMap<NodeId, DoorLockState>>,
}

impl DoorLockClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `LockDoor` command.
    ///
    /// # Upstream: src/app/clusters/door-lock-server/door-lock-server.cpp::lockDoor
    pub fn lock_door(&self, node: NodeId, pin: Option<&[u8]>) -> Result<DoorLockState> {
        Self::validate_pin(pin)?;
        self.state.lock().insert(node, DoorLockState::Locked);
        Ok(DoorLockState::Locked)
    }

    /// `UnlockDoor` command.
    ///
    /// # Upstream: src/app/clusters/door-lock-server/door-lock-server.cpp::unlockDoor
    pub fn unlock_door(&self, node: NodeId, pin: Option<&[u8]>) -> Result<DoorLockState> {
        Self::validate_pin(pin)?;
        self.state.lock().insert(node, DoorLockState::Unlocked);
        Ok(DoorLockState::Unlocked)
    }

    /// Read the cached lock state.
    pub fn read_state(&self, node: NodeId) -> Result<DoorLockState> {
        self.state
            .lock()
            .get(&node)
            .copied()
            .ok_or_else(|| MatterError::NotFound(format!("door lock {:?}", node)))
    }

    /// Manufacturer-side update.
    pub fn report_state(&self, node: NodeId, state: DoorLockState) {
        self.state.lock().insert(node, state);
    }

    fn validate_pin(pin: Option<&[u8]>) -> Result<()> {
        if let Some(p) = pin {
            if !(4..=10).contains(&p.len()) {
                return Err(MatterError::InvalidArgument(format!(
                    "PIN length {} not in [4,10]",
                    p.len()
                )));
            }
            if !p.iter().all(|b| b.is_ascii_digit()) {
                return Err(MatterError::InvalidArgument(
                    "PIN must be ASCII digits".into(),
                ));
            }
        }
        Ok(())
    }
}

impl ClusterClient for DoorLockClient {
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

    /// # Upstream: src/app/tests/cluster-objects/TestDoorLock.cpp::TestLockDoor
    #[test]
    fn lock_door_transitions_state() {
        let c = DoorLockClient::new();
        let n = NodeId(1);
        assert_eq!(c.lock_door(n, None).expect("lock"), DoorLockState::Locked);
        assert_eq!(c.read_state(n).expect("read"), DoorLockState::Locked);
        assert_eq!(c.unlock_door(n, None).expect("unlock"), DoorLockState::Unlocked);
        assert_eq!(c.read_state(n).expect("read"), DoorLockState::Unlocked);
    }

    #[test]
    fn pin_validates_length() {
        let c = DoorLockClient::new();
        assert!(c.lock_door(NodeId(1), Some(b"123")).is_err());
        assert!(c.lock_door(NodeId(1), Some(b"12345678901")).is_err());
        assert!(c.lock_door(NodeId(1), Some(b"1234")).is_ok());
    }

    #[test]
    fn pin_validates_digits() {
        let c = DoorLockClient::new();
        assert!(c.lock_door(NodeId(1), Some(b"abcd")).is_err());
        assert!(c.lock_door(NodeId(1), Some(b"1234")).is_ok());
    }

    #[test]
    fn report_state_overrides_internal_view() {
        let c = DoorLockClient::new();
        let n = NodeId(7);
        c.report_state(n, DoorLockState::NotFullyLocked);
        assert_eq!(c.read_state(n).expect("read"), DoorLockState::NotFullyLocked);
    }
}
