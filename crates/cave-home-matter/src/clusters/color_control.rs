// SPDX-License-Identifier: Apache-2.0
//! ColorControl cluster (0x0300) client.
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/app/clusters/color-control-server/color-control-server.cpp

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::clusters::ClusterClient;
use crate::error::{MatterError, Result};
use crate::fabric::NodeId;

/// Matter cluster id.
pub const CLUSTER_ID: u32 = 0x0300;

/// Per-node color state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ColorState {
    pub hue: u8,
    pub saturation: u8,
    pub color_temp_mireds: u16,
}

/// ColorControl client.
#[derive(Debug, Default)]
pub struct ColorControlClient {
    state: Mutex<BTreeMap<NodeId, ColorState>>,
}

impl ColorControlClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue `MoveToHueAndSaturation`.
    ///
    /// # Upstream: src/app/clusters/color-control-server/color-control-server.cpp::moveToHueAndSaturation
    pub fn move_to_hue_and_saturation(
        &self,
        node: NodeId,
        hue: u8,
        saturation: u8,
        _transition_time_ds: u16,
    ) -> Result<()> {
        if saturation > 254 {
            return Err(MatterError::InvalidArgument(format!(
                "saturation {saturation} > 254"
            )));
        }
        let mut s = self.state.lock();
        let entry = s.entry(node).or_default();
        entry.hue = hue;
        entry.saturation = saturation;
        Ok(())
    }

    /// Issue `MoveToColorTemperature`.
    ///
    /// # Upstream: src/app/clusters/color-control-server/color-control-server.cpp::moveToColorTemperature
    pub fn move_to_color_temperature(
        &self,
        node: NodeId,
        mireds: u16,
        _transition_time_ds: u16,
    ) -> Result<()> {
        // 1..0xFFEF inclusive per spec range.
        if !(1..=0xFFEF).contains(&mireds) {
            return Err(MatterError::InvalidArgument(format!(
                "color temperature mireds {mireds} out of range"
            )));
        }
        let mut s = self.state.lock();
        let entry = s.entry(node).or_default();
        entry.color_temp_mireds = mireds;
        Ok(())
    }

    /// Read the cached color state.
    pub fn read_state(&self, node: NodeId) -> Result<ColorState> {
        self.state
            .lock()
            .get(&node)
            .copied()
            .ok_or_else(|| MatterError::NotFound(format!("color state for {:?}", node)))
    }
}

impl ClusterClient for ColorControlClient {
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

    /// # Upstream: src/app/tests/cluster-objects/TestColorControl.cpp::TestMoveToHueAndSaturation
    #[test]
    fn move_to_hue_sat_records_target() {
        let c = ColorControlClient::new();
        let n = NodeId(1);
        c.move_to_hue_and_saturation(n, 120, 200, 0).expect("move");
        let st = c.read_state(n).expect("read");
        assert_eq!(st.hue, 120);
        assert_eq!(st.saturation, 200);
    }

    #[test]
    fn move_to_color_temperature_records_target() {
        let c = ColorControlClient::new();
        let n = NodeId(1);
        c.move_to_color_temperature(n, 153, 0).expect("move"); // ~6500K
        let st = c.read_state(n).expect("read");
        assert_eq!(st.color_temp_mireds, 153);
    }

    #[test]
    fn move_to_color_temperature_rejects_zero() {
        let c = ColorControlClient::new();
        assert!(c.move_to_color_temperature(NodeId(1), 0, 0).is_err());
    }

    #[test]
    fn move_to_hue_sat_rejects_oversaturated() {
        let c = ColorControlClient::new();
        assert!(c
            .move_to_hue_and_saturation(NodeId(1), 0, 255, 0)
            .is_err());
    }
}
