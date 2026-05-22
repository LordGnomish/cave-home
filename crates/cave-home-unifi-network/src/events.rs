// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi/hub/websocket.py
//
// The HA UniFi integration consumes a WebSocket stream from the
// controller (`aiounifi.websocket.WebSocketSignal`) and fans events
// out via HA dispatchers. cave-home replaces dispatchers with a typed
// `ControllerEvent` enum and a `broadcast::Sender` (Phase 1: model
// shape; Phase 2: wire the actual decoder).

use serde::{Deserialize, Serialize};

use crate::identifiers::{ClientId, DeviceId};

/// Events emitted by the UniFi controller WebSocket.
///
/// HA exposes these as bus events (`unifi_*`) once they reach the
/// frontend. cave-home strips the prefix and turns them into typed
/// variants for the automation engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControllerEvent {
    /// A previously-known client just connected to the network.
    ClientConnected {
        /// Client MAC.
        client: ClientId,
    },
    /// A client went offline.
    ClientDisconnected {
        /// Client MAC.
        client: ClientId,
    },
    /// A device started a firmware upgrade. HA event:
    /// `unifi_device_upgrade`.
    DeviceUpgrade {
        /// Device MAC.
        device: DeviceId,
        /// Firmware version before the upgrade.
        from: String,
        /// Firmware version after the upgrade.
        to: String,
    },
    /// A device-port PoE state changed. HA event:
    /// `unifi_port_poe_state`.
    PortPoeChange {
        /// Device MAC.
        device: DeviceId,
        /// 1-based port index.
        port: u32,
        /// True if PoE is now enabled on this port.
        enabled: bool,
    },
    /// A device transitioned to a different connection state.
    DeviceStateChange {
        /// Device MAC.
        device: DeviceId,
        /// New state string (HA `DEVICE_STATES` value).
        new_state: String,
    },
}

impl ControllerEvent {
    /// True if the event represents a client-roam transition (connect or
    /// disconnect). Used by the automation engine to debounce roaming.
    #[must_use]
    pub fn is_client_roam(&self) -> bool {
        matches!(
            self,
            Self::ClientConnected { .. } | Self::ClientDisconnected { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_events_are_roams() {
        let e = ControllerEvent::ClientConnected {
            client: ClientId::new("aa:bb"),
        };
        assert!(e.is_client_roam());
        let e = ControllerEvent::ClientDisconnected {
            client: ClientId::new("aa:bb"),
        };
        assert!(e.is_client_roam());
    }

    #[test]
    fn device_events_are_not_roams() {
        let e = ControllerEvent::DeviceUpgrade {
            device: DeviceId::new("aa:bb"),
            from: "1".into(),
            to: "2".into(),
        };
        assert!(!e.is_client_roam());
    }
}
