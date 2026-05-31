// SPDX-License-Identifier: Apache-2.0
//! LevelControl cluster (0x0008) client.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/app/clusters/level-control/level-control.cpp

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::clusters::ClusterClient;
use crate::error::{MatterError, Result};
use crate::fabric::NodeId;

/// Matter cluster id.
pub const CLUSTER_ID: u32 = 0x0008;

/// Range checking constants from level-control.cpp.
pub const LEVEL_MIN: u8 = 0;
pub const LEVEL_MAX: u8 = 254;

/// MoveToLevel command client.
#[derive(Debug, Default)]
pub struct LevelControlClient {
    state: Mutex<BTreeMap<NodeId, u8>>,
}

impl LevelControlClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue MoveToLevel.
    ///
    /// # Upstream: src/app/clusters/level-control/level-control.cpp::moveToLevelHandler
    pub fn move_to_level(
        &self,
        node: NodeId,
        level: u8,
        _transition_time_ds: u16,
    ) -> Result<()> {
        if level > LEVEL_MAX {
            return Err(MatterError::InvalidArgument(format!(
                "level {level} > {LEVEL_MAX}"
            )));
        }
        self.state.lock().insert(node, level);
        Ok(())
    }

    /// Read the cached level attribute.
    pub fn read_level(&self, node: NodeId) -> Result<u8> {
        self.state
            .lock()
            .get(&node)
            .copied()
            .ok_or_else(|| MatterError::NotFound(format!("level for {:?}", node)))
    }
}

impl ClusterClient for LevelControlClient {
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

    /// # Upstream: src/app/tests/cluster-objects/TestLevelControl.cpp::TestMoveToLevel
    #[test]
    fn move_to_level_records_target() {
        let c = LevelControlClient::new();
        let n = NodeId(1);
        c.move_to_level(n, 128, 5).expect("move");
        assert_eq!(c.read_level(n).expect("read"), 128);
    }

    #[test]
    fn move_to_level_rejects_out_of_range() {
        let c = LevelControlClient::new();
        let err = c
            .move_to_level(NodeId(1), 255, 0)
            .expect_err("must reject");
        match err {
            MatterError::InvalidArgument(_) => {}
            other => panic!("unexpected error {other:?}"),
        }
    }
}
