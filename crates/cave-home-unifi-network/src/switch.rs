// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi/switch.py
//
// HA `switch.py` exposes three switch classes:
//   - UnifiBlockClientSwitch   (block/unblock a client by MAC)
//   - UnifiDPIRestrictionGroupSwitch (DPI group toggle)
//   - UnifiOutletSwitch        (PoE / outlet relay toggle on a device port)
//
// cave-home Phase 1 ports the two switch surfaces grandma cares about
// (block + outlet). DPI is a Phase 2 ticket.

use serde::{Deserialize, Serialize};

use crate::identifiers::{ClientId, DeviceId};

/// Block / unblock switch for a single client MAC.
///
/// Source: `UnifiBlockClientSwitch` in `switch.py`. HA's
/// `async_turn_on` calls `controller.api.clients.async_block` /
/// `_unblock` against the controller; cave-home Phase 1 records the
/// desired state — the wire-side call lands in
/// `UnifiController::apply_block_state()` (Phase 2 ticket).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockSwitch {
    /// Client MAC this switch operates on.
    pub client: ClientId,
    /// Friendly label shown in the portal (HA: client.name / hostname).
    pub label: String,
    /// True if the client is currently blocked.
    pub blocked: bool,
}

impl BlockSwitch {
    /// Construct a switch in the "not blocked" state.
    #[must_use]
    pub fn new(client: ClientId, label: impl Into<String>) -> Self {
        Self {
            client,
            label: label.into(),
            blocked: false,
        }
    }

    /// Set the desired blocked state. (HA `async_turn_on` /
    /// `async_turn_off`.)
    pub fn set_blocked(&mut self, blocked: bool) {
        self.blocked = blocked;
    }
}

/// Outlet / PoE relay switch on a UniFi device port (HA:
/// `UnifiOutletSwitch`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutletSwitch {
    /// Owning device MAC.
    pub device: DeviceId,
    /// Outlet index (1-based; matches UniFi controller port numbering).
    pub outlet_idx: u32,
    /// Friendly label (HA: outlet.name).
    pub label: String,
    /// True if the relay is currently closed (delivering power).
    pub relay_state: bool,
}

impl OutletSwitch {
    /// Construct an outlet switch in the "off" state.
    #[must_use]
    pub fn new(device: DeviceId, outlet_idx: u32, label: impl Into<String>) -> Self {
        Self {
            device,
            outlet_idx,
            label: label.into(),
            relay_state: false,
        }
    }

    /// Set the relay state.
    pub fn set_relay_state(&mut self, on: bool) {
        self.relay_state = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_switch_default_unblocked() {
        let sw = BlockSwitch::new(ClientId::new("aa:bb"), "child");
        assert!(!sw.blocked);
    }

    #[test]
    fn outlet_switch_toggle() {
        let mut o = OutletSwitch::new(DeviceId::new("aa:bb"), 1, "outlet");
        o.set_relay_state(true);
        assert!(o.relay_state);
        o.set_relay_state(false);
        assert!(!o.relay_state);
    }
}
