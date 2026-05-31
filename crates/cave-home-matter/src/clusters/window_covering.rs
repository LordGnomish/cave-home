// SPDX-License-Identifier: Apache-2.0
//! WindowCovering cluster (0x0102) client.
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/app/clusters/window-covering-server/window-covering-server.cpp
//!
//! Roller-shutter / blind control from the commissioner perspective.
//! Positions use the Matter `Percent100ths` convention: `0` is fully
//! open (covering retracted, window clear) and `10000` (= 100.00 %) is
//! fully closed. The commissioner records the commanded target and the
//! derived [`OperationalStatus`]; the accessory confirms motion via
//! [`WindowCoveringClient::report_state`].

use std::cmp::Ordering;
use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::clusters::ClusterClient;
use crate::error::{MatterError, Result};
use crate::fabric::NodeId;

/// Matter cluster id.
pub const CLUSTER_ID: u32 = 0x0102;

/// Maximum `Percent100ths` value (= 100.00 %, fully closed).
///
/// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::WC_PERCENT100THS_MAX_CLOSED
pub const PERCENT100THS_MAX: u16 = 10000;

/// Per-axis motion state.
///
/// # Upstream: src/app/clusters/window-covering-server/window-covering-server.h::OperationalState
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OperationalStatus {
    /// Covering is at rest (`OperationalState::Stall`).
    #[default]
    Stopped,
    /// Moving toward fully open / lower percentage (`MovingUpOrOpen`).
    Opening,
    /// Moving toward fully closed / higher percentage (`MovingDownOrClose`).
    Closing,
}

/// Cached attribute view for one covering.
///
/// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp attributes
/// `CurrentPositionLiftPercent100ths` / `CurrentPositionTiltPercent100ths` /
/// `OperationalStatus`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoveringState {
    /// `CurrentPositionLiftPercent100ths` (0 = open, 10000 = closed).
    pub lift_percent100ths: u16,
    /// `CurrentPositionTiltPercent100ths` (0 = open, 10000 = closed).
    pub tilt_percent100ths: u16,
    /// Derived `OperationalStatus`.
    pub status: OperationalStatus,
}

/// WindowCovering client.
#[derive(Debug, Default)]
pub struct WindowCoveringClient {
    state: Mutex<BTreeMap<NodeId, CoveringState>>,
}

impl WindowCoveringClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `UpOrOpen` command — drive the lift fully open (0 %).
    ///
    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::UpOrOpen
    pub fn up_or_open(&self, node: NodeId) -> Result<CoveringState> {
        self.go_to_lift_percentage(node, 0)
    }

    /// `DownOrClose` command — drive the lift fully closed (100 %).
    ///
    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::DownOrClose
    pub fn down_or_close(&self, node: NodeId) -> Result<CoveringState> {
        self.go_to_lift_percentage(node, PERCENT100THS_MAX)
    }

    /// `StopMotion` command — halt the covering, preserving position.
    ///
    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::StopMotion
    pub fn stop_motion(&self, node: NodeId) -> Result<CoveringState> {
        let mut map = self.state.lock();
        let entry = map.entry(node).or_default();
        entry.status = OperationalStatus::Stopped;
        Ok(*entry)
    }

    /// `GoToLiftPercentage` command — move the lift to an absolute target.
    ///
    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::GoToLiftPercentage
    ///
    /// # Errors
    /// [`MatterError::InvalidArgument`] if `percent100ths` exceeds
    /// [`PERCENT100THS_MAX`].
    pub fn go_to_lift_percentage(
        &self,
        node: NodeId,
        percent100ths: u16,
    ) -> Result<CoveringState> {
        Self::validate_percent(percent100ths)?;
        let mut map = self.state.lock();
        let entry = map.entry(node).or_default();
        entry.status = Self::direction(entry.lift_percent100ths, percent100ths);
        entry.lift_percent100ths = percent100ths;
        Ok(*entry)
    }

    /// `GoToTiltPercentage` command — move the tilt to an absolute target.
    ///
    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::GoToTiltPercentage
    ///
    /// # Errors
    /// [`MatterError::InvalidArgument`] if `percent100ths` exceeds
    /// [`PERCENT100THS_MAX`].
    pub fn go_to_tilt_percentage(
        &self,
        node: NodeId,
        percent100ths: u16,
    ) -> Result<CoveringState> {
        Self::validate_percent(percent100ths)?;
        let mut map = self.state.lock();
        let entry = map.entry(node).or_default();
        entry.status = Self::direction(entry.tilt_percent100ths, percent100ths);
        entry.tilt_percent100ths = percent100ths;
        Ok(*entry)
    }

    /// Read the cached covering state.
    ///
    /// # Errors
    /// [`MatterError::NotFound`] if no command/report has been seen for `node`.
    pub fn read_state(&self, node: NodeId) -> Result<CoveringState> {
        self.state
            .lock()
            .get(&node)
            .copied()
            .ok_or_else(|| MatterError::NotFound(format!("window covering {:?}", node)))
    }

    /// Accessory-side update of the cached state (motion completion report).
    pub fn report_state(&self, node: NodeId, state: CoveringState) {
        self.state.lock().insert(node, state);
    }

    /// Lower target percentage = moving toward open; higher = toward closed.
    fn direction(current: u16, target: u16) -> OperationalStatus {
        match target.cmp(&current) {
            Ordering::Less => OperationalStatus::Opening,
            Ordering::Greater => OperationalStatus::Closing,
            Ordering::Equal => OperationalStatus::Stopped,
        }
    }

    fn validate_percent(percent100ths: u16) -> Result<()> {
        if percent100ths > PERCENT100THS_MAX {
            return Err(MatterError::InvalidArgument(format!(
                "percent100ths {percent100ths} > {PERCENT100THS_MAX}"
            )));
        }
        Ok(())
    }
}

impl ClusterClient for WindowCoveringClient {
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
    use crate::error::MatterError;
    use crate::fabric::NodeId;

    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::UpOrOpen
    #[test]
    fn up_or_open_drives_toward_open() {
        let c = WindowCoveringClient::new();
        let n = NodeId(1);
        // Start fully closed, then command up/open.
        c.down_or_close(n).expect("close");
        let st = c.up_or_open(n).expect("open");
        assert_eq!(st.lift_percent100ths, 0);
        assert_eq!(st.status, OperationalStatus::Opening);
    }

    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::DownOrClose
    #[test]
    fn down_or_close_drives_toward_closed() {
        let c = WindowCoveringClient::new();
        let n = NodeId(2);
        let st = c.down_or_close(n).expect("close");
        assert_eq!(st.lift_percent100ths, PERCENT100THS_MAX);
        assert_eq!(st.status, OperationalStatus::Closing);
    }

    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::GoToLiftPercentage
    #[test]
    fn go_to_lift_percentage_records_target() {
        let c = WindowCoveringClient::new();
        let n = NodeId(3);
        let st = c.go_to_lift_percentage(n, 5000).expect("go");
        assert_eq!(st.lift_percent100ths, 5000);
        assert_eq!(c.read_state(n).expect("read").lift_percent100ths, 5000);
    }

    #[test]
    fn go_to_lift_percentage_rejects_out_of_range() {
        let c = WindowCoveringClient::new();
        let err = c
            .go_to_lift_percentage(NodeId(4), PERCENT100THS_MAX + 1)
            .expect_err("must reject");
        match err {
            MatterError::InvalidArgument(_) => {}
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn go_to_lift_percentage_direction_is_closing_when_increasing() {
        let c = WindowCoveringClient::new();
        let n = NodeId(5);
        // Default is fully open (0); moving to 3000 closes.
        let st = c.go_to_lift_percentage(n, 3000).expect("go");
        assert_eq!(st.status, OperationalStatus::Closing);
    }

    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::GoToTiltPercentage
    #[test]
    fn go_to_tilt_percentage_records_target() {
        let c = WindowCoveringClient::new();
        let n = NodeId(6);
        let st = c.go_to_tilt_percentage(n, 2500).expect("go");
        assert_eq!(st.tilt_percent100ths, 2500);
    }

    /// # Upstream: src/app/clusters/window-covering-server/window-covering-server.cpp::StopMotion
    #[test]
    fn stop_motion_halts_motion() {
        let c = WindowCoveringClient::new();
        let n = NodeId(7);
        c.down_or_close(n).expect("close");
        let st = c.stop_motion(n).expect("stop");
        assert_eq!(st.status, OperationalStatus::Stopped);
        // Position is preserved across a stop.
        assert_eq!(st.lift_percent100ths, PERCENT100THS_MAX);
    }

    #[test]
    fn read_state_unknown_node_errors() {
        let c = WindowCoveringClient::new();
        assert!(c.read_state(NodeId(99)).is_err());
    }

    #[test]
    fn report_state_overrides_internal_view() {
        let c = WindowCoveringClient::new();
        let n = NodeId(8);
        let reported = CoveringState {
            lift_percent100ths: 1234,
            tilt_percent100ths: 4321,
            status: OperationalStatus::Stopped,
        };
        c.report_state(n, reported);
        assert_eq!(c.read_state(n).expect("read"), reported);
    }

    #[test]
    fn cluster_id_is_window_covering() {
        use crate::clusters::ClusterClient;
        assert_eq!(WindowCoveringClient::new().cluster_id(), 0x0102);
    }
}
