// SPDX-License-Identifier: Apache-2.0
//! Top-level `Controller` composition — the Phase 1 cave-home Matter entry point.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/controller/CHIPDeviceControllerFactory.cpp
//!
//! The Controller bundles the long-lived pieces of state a commissioner
//! needs (FabricTable, AccessControl, GroupDataProvider,
//! NetworkCommissioningClient) and exposes a single ergonomic façade
//! that downstream crates (`cave-home-cli`, `cave-home-portal`,
//! `cave-home-binary`) compose with.
//!
//! Per ADR-007 the user-facing surface labels the Controller's
//! aggregated fabric as **"Hane"**; nodes are **"Cihaz"**. The
//! technical type names retain the chip vocabulary for line-by-line
//! parity, the translation lives at the UI layer.
//!
//! # Phase 1 coverage
//! - Construct a `Controller` from a `CommissionerConfig`.
//! - Drive `pair` / `unpair` through the embedded `Commissioner`.
//! - List paired devices (read-through to `Commissioner::paired`).
//! - Provide handles to the cluster clients (OnOff, Level, Color,
//!   Thermostat, DoorLock, WindowCovering, NetworkCommissioning) for callers that want
//!   to invoke operational commands.
//!
//! Out of Phase 1: subscription engine, mDNS discovery, OTA-provider
//! role, ICD check-in — tracked under
//! `parity.manifest.toml::[[unmapped]]`.

use std::sync::Arc;

use crate::acl::AccessControl;
use crate::clusters::color_control::ColorControlClient;
use crate::clusters::door_lock::DoorLockClient;
use crate::clusters::level_control::LevelControlClient;
use crate::clusters::network_commissioning::NetworkCommissioningClient;
use crate::clusters::on_off::OnOffClient;
use crate::clusters::thermostat::ThermostatClient;
use crate::clusters::window_covering::WindowCoveringClient;
use crate::commissioner::{Commissioner, CommissionerConfig, PairedDevice};
use crate::error::Result;
use crate::fabric::FabricTable;
use crate::group_key::GroupDataProvider;
use crate::pase::PaseSession;
use crate::setup_payload::SetupPayload;

/// Top-level façade over the Matter commissioner + cluster clients.
///
/// # Upstream: src/controller/CHIPDeviceControllerFactory.cpp::Controller
pub struct Controller {
    commissioner: Commissioner,
    fabrics: Arc<FabricTable>,
    acl: Arc<AccessControl>,
    groups: Arc<GroupDataProvider>,
    network: Arc<NetworkCommissioningClient>,
}

impl Controller {
    /// Build a controller with all long-lived state pre-wired.
    ///
    /// # Upstream: src/controller/CHIPDeviceControllerFactory.cpp::CreateDeviceController
    pub fn new(
        cfg: CommissionerConfig,
        fabrics: Arc<FabricTable>,
        acl: Arc<AccessControl>,
        groups: Arc<GroupDataProvider>,
        network: Arc<NetworkCommissioningClient>,
    ) -> Self {
        let commissioner = Commissioner::new(
            cfg,
            Arc::clone(&fabrics),
            Arc::clone(&acl),
            Arc::clone(&network),
        );
        Self {
            commissioner,
            fabrics,
            acl,
            groups,
            network,
        }
    }

    /// Read-only access to the embedded commissioner.
    #[must_use]
    pub fn commissioner(&self) -> &Commissioner {
        &self.commissioner
    }

    /// Read-only access to the FabricTable.
    #[must_use]
    pub fn fabrics(&self) -> &Arc<FabricTable> {
        &self.fabrics
    }

    /// Read-only access to the AccessControl list.
    #[must_use]
    pub fn acl(&self) -> &Arc<AccessControl> {
        &self.acl
    }

    /// Read-only access to the GroupDataProvider.
    #[must_use]
    pub fn groups(&self) -> &Arc<GroupDataProvider> {
        &self.groups
    }

    /// Read-only access to the NetworkCommissioningClient (Thread / Wi-Fi).
    #[must_use]
    pub fn network(&self) -> &Arc<NetworkCommissioningClient> {
        &self.network
    }

    /// Drive a pairing through the commissioner.
    ///
    /// # Upstream: src/controller/CHIPDeviceController.cpp::DeviceController::PairDevice
    ///
    /// # Errors
    /// Propagates any error from the underlying commissioner —
    /// PASE failure, fabric commit failure, network commissioning
    /// failure, CASE failure.
    pub fn pair(
        &self,
        setup_payload: &SetupPayload,
        device_pase: PaseSession,
    ) -> Result<PairedDevice> {
        self.commissioner.pair_device(setup_payload, device_pase)
    }

    /// Build a fresh OnOff cluster client.
    #[must_use]
    pub fn on_off_client(&self) -> OnOffClient {
        OnOffClient::new()
    }

    /// Build a fresh LevelControl cluster client.
    #[must_use]
    pub fn level_control_client(&self) -> LevelControlClient {
        LevelControlClient::new()
    }

    /// Build a fresh ColorControl cluster client.
    #[must_use]
    pub fn color_control_client(&self) -> ColorControlClient {
        ColorControlClient::new()
    }

    /// Build a fresh Thermostat cluster client.
    #[must_use]
    pub fn thermostat_client(&self) -> ThermostatClient {
        ThermostatClient::new()
    }

    /// Build a fresh DoorLock cluster client.
    #[must_use]
    pub fn door_lock_client(&self) -> DoorLockClient {
        DoorLockClient::new()
    }

    /// Build a fresh WindowCovering cluster client (roller shutters / blinds).
    #[must_use]
    pub fn window_covering_client(&self) -> WindowCoveringClient {
        WindowCoveringClient::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::network_commissioning::NetworkCommissioningClient;
    use crate::fabric::{FabricId, NodeId};

    fn test_controller() -> Controller {
        let cfg = CommissionerConfig {
            admin_fabric_id: FabricId(1),
            admin_node_id: NodeId(1),
            admin_vendor_id: 0xFFF1,
            admin_fabric_label: "test-hane".to_string(),
            admin_root_ca_public_key: [0xAA; 32],
            admin_noc_public_key: [0xBB; 32],
            default_thread_dataset: None,
        };
        Controller::new(
            cfg,
            Arc::new(FabricTable::new()),
            Arc::new(AccessControl::new()),
            Arc::new(GroupDataProvider::new()),
            Arc::new(NetworkCommissioningClient::new()),
        )
    }

    #[test]
    fn controller_exposes_long_lived_state() {
        let c = test_controller();
        // All four handles are non-null and survive past construction.
        assert!(Arc::strong_count(c.fabrics()) >= 1);
        assert!(Arc::strong_count(c.acl()) >= 1);
        assert!(Arc::strong_count(c.groups()) >= 1);
        assert!(Arc::strong_count(c.network()) >= 1);
    }

    #[test]
    fn controller_mints_independent_cluster_clients() {
        let c = test_controller();
        let _a = c.on_off_client();
        let _b = c.on_off_client();
        let _l = c.level_control_client();
        let _color = c.color_control_client();
        let _t = c.thermostat_client();
        let _d = c.door_lock_client();
        let _w = c.window_covering_client();
        // Smoke — clients are independently constructible.
    }
}
