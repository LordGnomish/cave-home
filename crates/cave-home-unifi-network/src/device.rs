// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi/const.py
//                            (DEVICE_STATES table) +
//                            aiounifi/models/device.py (DeviceState enum).
//
// `DEVICE_STATES` in HA is a `dict[DeviceState, str]` mapping the upstream
// `aiounifi.models.device.DeviceState` enum to the lowercase strings the
// integration ships to the Lovelace frontend. cave-home keeps the same
// 12-variant enum but writes it natively in Rust.

use serde::{Deserialize, Serialize};

use crate::const_table::ATTR_MANUFACTURER;
use crate::identifiers::DeviceId;

/// UniFi device connection state (HA: `aiounifi.models.device.DeviceState`).
///
/// HA `DEVICE_STATES` table is reproduced 1:1 by `as_str()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceState {
    /// `disconnected`
    Disconnected,
    /// `connected`
    Connected,
    /// `pending`
    Pending,
    /// `firmware_mismatch`
    FirmwareMismatch,
    /// `upgrading`
    Upgrading,
    /// `provisioning`
    Provisioning,
    /// `heartbeat_missed`
    HeartbeatMissed,
    /// `adopting`
    Adopting,
    /// `deleting`
    Deleting,
    /// `inform_error`
    InformError,
    /// `adoption_failed` (HA spelt this as `ADOPTION_FALIED`; cave-home
    /// keeps the user-facing string from the dict value, not the typo'd
    /// enum variant name).
    AdoptionFailed,
    /// `isolated`
    Isolated,
}

impl DeviceState {
    /// Lower-case string as shipped to the frontend (HA `DEVICE_STATES`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connected => "connected",
            Self::Pending => "pending",
            Self::FirmwareMismatch => "firmware_mismatch",
            Self::Upgrading => "upgrading",
            Self::Provisioning => "provisioning",
            Self::HeartbeatMissed => "heartbeat_missed",
            Self::Adopting => "adopting",
            Self::Deleting => "deleting",
            Self::InformError => "inform_error",
            Self::AdoptionFailed => "adoption_failed",
            Self::Isolated => "isolated",
        }
    }

    /// Parse the lowercase string back into a variant.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "disconnected" => Self::Disconnected,
            "connected" => Self::Connected,
            "pending" => Self::Pending,
            "firmware_mismatch" => Self::FirmwareMismatch,
            "upgrading" => Self::Upgrading,
            "provisioning" => Self::Provisioning,
            "heartbeat_missed" => Self::HeartbeatMissed,
            "adopting" => Self::Adopting,
            "deleting" => Self::Deleting,
            "inform_error" => Self::InformError,
            "adoption_failed" => Self::AdoptionFailed,
            "isolated" => Self::Isolated,
            _ => return None,
        })
    }

    /// Iterate every variant.
    #[must_use]
    pub fn all() -> [Self; 12] {
        [
            Self::Disconnected,
            Self::Connected,
            Self::Pending,
            Self::FirmwareMismatch,
            Self::Upgrading,
            Self::Provisioning,
            Self::HeartbeatMissed,
            Self::Adopting,
            Self::Deleting,
            Self::InformError,
            Self::AdoptionFailed,
            Self::Isolated,
        ]
    }

    /// `connected` is the only "healthy / ready" state — every other
    /// variant means the device cannot serve clients. Used by the
    /// portal to colour the device tile.
    #[must_use]
    pub fn is_healthy(self) -> bool {
        matches!(self, Self::Connected)
    }
}

/// Family of UniFi device. Drives the portal label vocabulary.
///
/// Source: aiounifi `Device.type` field (`"usw"`, `"uap"`, `"ugw"`,
/// `"udm"`) + HA's `_attr_device_info["manufacturer"]` rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    /// `usw` — UniFi Switch.
    Switch,
    /// `uap` — UniFi Access Point.
    AccessPoint,
    /// `ugw` — UniFi Security Gateway.
    Gateway,
    /// `udm` / `udmpro` — UniFi Dream Machine (router + controller).
    DreamMachine,
    /// Unknown / future device class.
    Other,
}

impl DeviceKind {
    /// Map the aiounifi `Device.type` string to the kind.
    #[must_use]
    pub fn from_type_string(t: &str) -> Self {
        match t {
            "usw" => Self::Switch,
            "uap" => Self::AccessPoint,
            "ugw" => Self::Gateway,
            "udm" | "udmpro" | "udmse" | "udr" | "udw" => Self::DreamMachine,
            _ => Self::Other,
        }
    }
}

/// Per-port telemetry (HA: `aiounifi.models.port.Port`).
///
/// HA renders these as `unifi_port_*` sensors (PoE state, rx/tx rate,
/// uplink flag). cave-home strips the API-jargon names and exposes only
/// what the portal needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortStat {
    /// 1-based port index on the switch.
    pub port_idx: u32,
    /// Cumulative receive bytes since last reset.
    pub rx_bytes: u64,
    /// Cumulative transmit bytes since last reset.
    pub tx_bytes: u64,
    /// True if this port is delivering PoE.
    pub poe_enabled: bool,
    /// True if this port is the switch's uplink to the gateway.
    pub is_uplink: bool,
}

impl PortStat {
    /// Construct a zero-traffic port stat for the given index.
    #[must_use]
    pub fn idle(port_idx: u32) -> Self {
        Self {
            port_idx,
            rx_bytes: 0,
            tx_bytes: 0,
            poe_enabled: false,
            is_uplink: false,
        }
    }

    /// Total bytes since last reset.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.rx_bytes.saturating_add(self.tx_bytes)
    }
}

/// UniFi network device (switch, AP, gateway).
///
/// Source: aiounifi `Device` model + HA `entity.py` device-info
/// construction. cave-home keeps the seven fields the portal needs;
/// the rest of the upstream model is intentionally not surfaced
/// (Phase 2 ticket: full Device parity).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnifiDevice {
    /// MAC-keyed device ID.
    pub id: DeviceId,
    /// User-set name from the UniFi controller (or model name if unset).
    pub label: String,
    /// Device family.
    pub kind: DeviceKind,
    /// Live state (HA `DEVICE_STATES`).
    pub state: DeviceState,
    /// Firmware version string.
    pub firmware: String,
    /// Per-port telemetry (empty for non-switch device kinds).
    pub ports: Vec<PortStat>,
    /// Manufacturer (always `"Ubiquiti Networks"` per HA
    /// `ATTR_MANUFACTURER`).
    pub manufacturer: &'static str,
}

impl UnifiDevice {
    /// Construct a freshly-discovered device in the `Pending` state.
    #[must_use]
    pub fn new(id: DeviceId, label: impl Into<String>, kind: DeviceKind) -> Self {
        Self {
            id,
            label: label.into(),
            kind,
            state: DeviceState::Pending,
            firmware: String::new(),
            ports: Vec::new(),
            manufacturer: ATTR_MANUFACTURER,
        }
    }
}

/// Translate a `DeviceKind` into the portal's home-world label.
/// Mandated by ADR-007 / Charter v6 §6.3.
#[must_use]
pub fn friendly_device_label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Switch => "Switch",
        DeviceKind::AccessPoint => "Wi-Fi noktası",
        DeviceKind::Gateway => "Yönlendirici",
        DeviceKind::DreamMachine => "UniFi Dream Machine",
        DeviceKind::Other => "Cihaz",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_kind_from_type_string() {
        assert_eq!(DeviceKind::from_type_string("usw"), DeviceKind::Switch);
        assert_eq!(DeviceKind::from_type_string("uap"), DeviceKind::AccessPoint);
        assert_eq!(DeviceKind::from_type_string("ugw"), DeviceKind::Gateway);
        assert_eq!(DeviceKind::from_type_string("udm"), DeviceKind::DreamMachine);
        assert_eq!(DeviceKind::from_type_string("udmpro"), DeviceKind::DreamMachine);
        assert_eq!(DeviceKind::from_type_string("future"), DeviceKind::Other);
    }

    #[test]
    fn connected_is_healthy() {
        assert!(DeviceState::Connected.is_healthy());
        assert!(!DeviceState::Adopting.is_healthy());
        assert!(!DeviceState::Disconnected.is_healthy());
    }

    #[test]
    fn unifi_device_construct_defaults() {
        let d = UnifiDevice::new(
            DeviceId::new("aa:bb:cc:dd:ee:00"),
            "Salon switch",
            DeviceKind::Switch,
        );
        assert_eq!(d.label, "Salon switch");
        assert_eq!(d.state, DeviceState::Pending);
        assert_eq!(d.manufacturer, "Ubiquiti Networks");
        assert!(d.ports.is_empty());
    }
}
