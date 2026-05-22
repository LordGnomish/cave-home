// SPDX-License-Identifier: Apache-2.0
//! OTA Requestor cluster client.
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/app/clusters/ota-requestor/DefaultOTARequestor.cpp
//!
//! Phase 1 ports the requestor state machine: QueryImage ->
//! ApplyUpdate -> Notify. The BDX bulk transfer that fetches the
//! image bytes is `[[unmapped]] phase-1b`.

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::error::{MatterError, Result};
use crate::fabric::NodeId;

/// OTA Requestor lifecycle states.
///
/// # Upstream: src/app/clusters/ota-requestor/OTARequestorInterface.h::OTAUpdateStateEnum
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OtaState {
    Unknown,
    Idle,
    Querying,
    Delayed,
    Downloading,
    Applying,
    DelayedOnApply,
    RollingBack,
    DelayedOnUserConsent,
}

/// Per-target requestor record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OtaTarget {
    pub node_id: NodeId,
    pub state: OtaState,
    pub current_version: u32,
    pub available_version: Option<u32>,
    pub last_query_count: u32,
    pub last_apply_count: u32,
}

/// OTA Requestor cluster client.
///
/// # Upstream: src/app/clusters/ota-requestor/DefaultOTARequestor.cpp::DefaultOTARequestor
#[derive(Debug, Default)]
pub struct OtaRequestor {
    state: Mutex<BTreeMap<NodeId, OtaTarget>>,
}

impl OtaRequestor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a target (paired Matter device) with its current firmware
    /// version.
    pub fn register(&self, node_id: NodeId, current_version: u32) {
        self.state.lock().insert(
            node_id,
            OtaTarget {
                node_id,
                state: OtaState::Idle,
                current_version,
                available_version: None,
                last_query_count: 0,
                last_apply_count: 0,
            },
        );
    }

    /// Trigger an immediate QueryImage on the device.
    ///
    /// # Upstream: src/app/clusters/ota-requestor/DefaultOTARequestor.cpp::TriggerImmediateQuery
    pub fn trigger_immediate_query(&self, node_id: NodeId) -> Result<OtaTarget> {
        let mut s = self.state.lock();
        let t = s
            .get_mut(&node_id)
            .ok_or_else(|| MatterError::NotFound(format!("ota target {:?}", node_id)))?;
        if !matches!(t.state, OtaState::Idle | OtaState::Delayed | OtaState::Unknown) {
            return Err(MatterError::IncorrectState(format!(
                "cannot query in state {:?}",
                t.state
            )));
        }
        t.state = OtaState::Querying;
        t.last_query_count = t.last_query_count.saturating_add(1);
        Ok(t.clone())
    }

    /// Record the device's QueryImageResponse — feeds available_version.
    pub fn record_query_response(&self, node_id: NodeId, version: u32) -> Result<()> {
        let mut s = self.state.lock();
        let t = s
            .get_mut(&node_id)
            .ok_or_else(|| MatterError::NotFound(format!("ota target {:?}", node_id)))?;
        if version <= t.current_version {
            t.state = OtaState::Idle;
            t.available_version = None;
            return Ok(());
        }
        t.state = OtaState::Downloading;
        t.available_version = Some(version);
        Ok(())
    }

    /// Apply the previously-downloaded update.
    ///
    /// # Upstream: src/app/clusters/ota-requestor/DefaultOTARequestor.cpp::ApplyUpdate
    pub fn apply_update(&self, node_id: NodeId) -> Result<OtaTarget> {
        let mut s = self.state.lock();
        let t = s
            .get_mut(&node_id)
            .ok_or_else(|| MatterError::NotFound(format!("ota target {:?}", node_id)))?;
        if !matches!(t.state, OtaState::Downloading) {
            return Err(MatterError::IncorrectState(format!(
                "cannot apply from {:?}",
                t.state
            )));
        }
        let Some(target_version) = t.available_version else {
            return Err(MatterError::IncorrectState(
                "no available version recorded".into(),
            ));
        };
        t.state = OtaState::Applying;
        t.last_apply_count = t.last_apply_count.saturating_add(1);
        t.current_version = target_version;
        t.available_version = None;
        Ok(t.clone())
    }

    /// NotifyUpdateApplied — transitions back to Idle.
    pub fn notify_update_applied(&self, node_id: NodeId) -> Result<()> {
        let mut s = self.state.lock();
        let t = s
            .get_mut(&node_id)
            .ok_or_else(|| MatterError::NotFound(format!("ota target {:?}", node_id)))?;
        t.state = OtaState::Idle;
        Ok(())
    }

    /// Snapshot of a target.
    pub fn target(&self, node_id: NodeId) -> Option<OtaTarget> {
        self.state.lock().get(&node_id).cloned()
    }

    /// Snapshot of all targets — used by the Portal admin page.
    pub fn iter_targets(&self) -> Vec<OtaTarget> {
        self.state.lock().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Upstream: src/app/tests/TestDefaultOTARequestor.cpp::TestQueryImage
    #[test]
    fn trigger_immediate_query_records_attempt() {
        let r = OtaRequestor::new();
        r.register(NodeId(1), 1);
        let target = r.trigger_immediate_query(NodeId(1)).expect("query");
        assert_eq!(target.state, OtaState::Querying);
        assert_eq!(target.last_query_count, 1);
    }

    /// # Upstream: src/app/tests/TestDefaultOTARequestor.cpp::TestApplyUpdate
    #[test]
    fn apply_update_transitions_state() {
        let r = OtaRequestor::new();
        r.register(NodeId(1), 1);
        r.trigger_immediate_query(NodeId(1)).expect("query");
        r.record_query_response(NodeId(1), 2).expect("response");
        let after_apply = r.apply_update(NodeId(1)).expect("apply");
        assert_eq!(after_apply.state, OtaState::Applying);
        assert_eq!(after_apply.current_version, 2);
        assert_eq!(after_apply.last_apply_count, 1);
        r.notify_update_applied(NodeId(1)).expect("notify");
        let final_state = r.target(NodeId(1)).expect("target");
        assert_eq!(final_state.state, OtaState::Idle);
        assert_eq!(final_state.current_version, 2);
    }

    #[test]
    fn query_response_lower_version_does_not_download() {
        let r = OtaRequestor::new();
        r.register(NodeId(1), 5);
        r.trigger_immediate_query(NodeId(1)).expect("query");
        r.record_query_response(NodeId(1), 4).expect("response");
        let target = r.target(NodeId(1)).expect("target");
        assert_eq!(target.state, OtaState::Idle);
        assert!(target.available_version.is_none());
    }

    #[test]
    fn apply_without_download_errors() {
        let r = OtaRequestor::new();
        r.register(NodeId(1), 1);
        assert!(r.apply_update(NodeId(1)).is_err());
    }

    #[test]
    fn unknown_target_errors() {
        let r = OtaRequestor::new();
        assert!(r.trigger_immediate_query(NodeId(99)).is_err());
    }
}
