// SPDX-License-Identifier: Apache-2.0
//! WindowCovering cluster (0x0102) client.
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/app/clusters/window-covering-server/window-covering-server.cpp

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
