// SPDX-License-Identifier: Apache-2.0
//! Thermostat cluster (0x0201) client.
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/app/clusters/thermostat-server/thermostat-server.cpp

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::clusters::ClusterClient;
use crate::error::{MatterError, Result};
use crate::fabric::NodeId;

/// Matter cluster id.
pub const CLUSTER_ID: u32 = 0x0201;

/// Mode argument for `SetpointRaiseLower`.
///
/// # Upstream: src/app/clusters/thermostat-server/thermostat-server.h::SetpointRaiseLowerModeEnum
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetpointMode {
    Heat,
    Cool,
    Both,
}

/// Per-node thermostat state.
///
/// Temperatures are stored as upstream's 0.01°C ticks (i16).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ThermostatState {
    pub heating_setpoint_centi_c: i16,
    pub cooling_setpoint_centi_c: i16,
    pub local_temperature_centi_c: i16,
}

/// Thermostat client.
#[derive(Debug)]
pub struct ThermostatClient {
    state: Mutex<BTreeMap<NodeId, ThermostatState>>,
    min_centi_c: i16,
    max_centi_c: i16,
}

impl Default for ThermostatClient {
    fn default() -> Self {
        Self {
            state: Mutex::new(BTreeMap::new()),
            // Reasonable household range: 7..35 C.
            min_centi_c: 700,
            max_centi_c: 3500,
        }
    }
}

impl ThermostatClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a thermostat node with initial setpoints.
    pub fn register(&self, node: NodeId, initial: ThermostatState) {
        self.state.lock().insert(node, initial);
    }

    /// `SetpointRaiseLower` — delta in 0.1°C.
    ///
    /// # Upstream: src/app/clusters/thermostat-server/thermostat-server.cpp::setpointRaiseLower
    pub fn setpoint_raise_lower(
        &self,
        node: NodeId,
        mode: SetpointMode,
        delta_tenths_c: i8,
    ) -> Result<ThermostatState> {
        let mut s = self.state.lock();
        let entry = s
            .get_mut(&node)
            .ok_or_else(|| MatterError::NotFound(format!("thermostat {:?}", node)))?;
        let delta_centi = i16::from(delta_tenths_c) * 10;
        if matches!(mode, SetpointMode::Heat | SetpointMode::Both) {
            entry.heating_setpoint_centi_c =
                clamp_setpoint(entry.heating_setpoint_centi_c.saturating_add(delta_centi), self.min_centi_c, self.max_centi_c);
        }
        if matches!(mode, SetpointMode::Cool | SetpointMode::Both) {
            entry.cooling_setpoint_centi_c =
                clamp_setpoint(entry.cooling_setpoint_centi_c.saturating_add(delta_centi), self.min_centi_c, self.max_centi_c);
        }
        Ok(*entry)
    }

    /// Read the cached state.
    pub fn read_state(&self, node: NodeId) -> Result<ThermostatState> {
        self.state
            .lock()
            .get(&node)
            .copied()
            .ok_or_else(|| MatterError::NotFound(format!("thermostat {:?}", node)))
    }

    /// Report a new local temperature reading from the device.
    pub fn record_local_temperature(&self, node: NodeId, centi_c: i16) -> Result<()> {
        let mut s = self.state.lock();
        let entry = s
            .get_mut(&node)
            .ok_or_else(|| MatterError::NotFound(format!("thermostat {:?}", node)))?;
        entry.local_temperature_centi_c = centi_c;
        Ok(())
    }
}

fn clamp_setpoint(v: i16, min: i16, max: i16) -> i16 {
    v.clamp(min, max)
}

impl ClusterClient for ThermostatClient {
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

    /// # Upstream: src/app/tests/cluster-objects/TestThermostat.cpp::TestSetpointRaiseLower
    #[test]
    fn setpoint_raise_lower_updates_setpoint() {
        let c = ThermostatClient::new();
        let n = NodeId(1);
        c.register(
            n,
            ThermostatState {
                heating_setpoint_centi_c: 2100,
                cooling_setpoint_centi_c: 2400,
                local_temperature_centi_c: 2050,
            },
        );
        let st = c
            .setpoint_raise_lower(n, SetpointMode::Heat, 5)
            .expect("raise");
        assert_eq!(st.heating_setpoint_centi_c, 2150);
        assert_eq!(st.cooling_setpoint_centi_c, 2400);
    }

    #[test]
    fn setpoint_clamps_to_household_range() {
        let c = ThermostatClient::new();
        let n = NodeId(1);
        c.register(
            n,
            ThermostatState {
                heating_setpoint_centi_c: 3400,
                cooling_setpoint_centi_c: 3500,
                local_temperature_centi_c: 0,
            },
        );
        let st = c
            .setpoint_raise_lower(n, SetpointMode::Both, 50)
            .expect("raise");
        assert_eq!(st.heating_setpoint_centi_c, 3500);
        assert_eq!(st.cooling_setpoint_centi_c, 3500);
    }

    #[test]
    fn unknown_node_errors() {
        let c = ThermostatClient::new();
        assert!(c
            .setpoint_raise_lower(NodeId(99), SetpointMode::Heat, 1)
            .is_err());
    }

    #[test]
    fn record_local_temperature_round_trips() {
        let c = ThermostatClient::new();
        let n = NodeId(1);
        c.register(n, ThermostatState::default());
        c.record_local_temperature(n, 2230).expect("record");
        let st = c.read_state(n).expect("read");
        assert_eq!(st.local_temperature_centi_c, 2230);
    }
}
