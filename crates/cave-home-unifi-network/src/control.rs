//! Control operations — the validated, pure actions a household performs.
//!
//! Every operation here is **pure**: it takes the current model, validates the
//! request, and either rejects it with a typed [`ControlError`] or produces a
//! typed [`Command`]. A [`Command`] is *what the phase-1b controller adapter
//! should send* — this crate never performs the I/O itself (network-bound,
//! deferred per ADR-009). Keeping the decision and the I/O apart is what makes
//! the engine testable with no controller present.
//!
//! Ports `HA` `unifi`'s switch-platform semantics (block-client switch, `PoE`
//! port-power switch, `WLAN` enable, port-forward toggle, device LED) as a
//! transport-free decision layer.

use crate::client::NetworkClient;
use crate::device::{DeviceKind, NetworkDevice};

/// `PoE` power mode for a switch port (ports `HA`'s `poe_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoeMode {
    /// Force power on.
    On,
    /// Force power off.
    Off,
    /// Auto-negotiate (the controller decides based on the attached device).
    Auto,
}

impl PoeMode {
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Auto => "auto",
        }
    }
}

/// A typed, validated control command — the decision, not the I/O.
///
/// A phase-1b controller adapter turns each variant into the matching local
/// `UniFi` Network `API` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Block a client from the network.
    BlockClient { mac: String },
    /// Unblock a previously-blocked client.
    UnblockClient { mac: String },
    /// Force a client to reconnect ("kick" it off Wi-Fi).
    ReconnectClient { mac: String },
    /// Set the `PoE` mode of a switch port.
    SetPoe { device_id: String, port: u16, mode: PoeMode },
    /// Enable or disable a wireless network.
    SetWlanEnabled { wlan: String, enabled: bool },
    /// Enable or disable a port-forward rule.
    SetPortForwardEnabled { rule: String, enabled: bool },
    /// Turn a device's status LED on or off.
    SetDeviceLed { device_id: String, on: bool },
}

/// Why a control operation was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlError {
    /// The client is already in the requested state (block / unblock no-op).
    AlreadyInState,
    /// The named port does not exist on the device.
    UnknownPort(u16),
    /// The port exists but cannot deliver `PoE`.
    PortNotPoeCapable(u16),
    /// `PoE` can only be controlled on a switch.
    NotASwitch,
    /// The operation needs a wired or wireless client but got the wrong kind.
    WrongConnectionKind,
}

impl core::fmt::Display for ControlError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AlreadyInState => f.write_str("client is already in the requested state"),
            Self::UnknownPort(n) => write!(f, "no port number {n} on this device"),
            Self::PortNotPoeCapable(n) => write!(f, "port {n} cannot deliver power"),
            Self::NotASwitch => f.write_str("power can only be set on a switch"),
            Self::WrongConnectionKind => f.write_str("operation not valid for this connection"),
        }
    }
}

impl std::error::Error for ControlError {}

/// Block a client. Rejected if the client is already blocked.
///
/// # Errors
/// [`ControlError::AlreadyInState`] if the client is already blocked.
pub fn block_client(client: &NetworkClient) -> Result<Command, ControlError> {
    if client.is_blocked() {
        return Err(ControlError::AlreadyInState);
    }
    Ok(Command::BlockClient { mac: client.mac().to_string() })
}

/// Unblock a client. Rejected if the client is not currently blocked.
///
/// # Errors
/// [`ControlError::AlreadyInState`] if the client is not blocked.
pub fn unblock_client(client: &NetworkClient) -> Result<Command, ControlError> {
    if !client.is_blocked() {
        return Err(ControlError::AlreadyInState);
    }
    Ok(Command::UnblockClient { mac: client.mac().to_string() })
}

/// Reconnect ("kick") a wireless client so it re-associates.
///
/// Only meaningful for a wireless client — a wired client cannot be kicked off
/// the air.
///
/// # Errors
/// [`ControlError::WrongConnectionKind`] if the client is wired.
pub fn reconnect_client(client: &NetworkClient) -> Result<Command, ControlError> {
    if !client.is_wireless() {
        return Err(ControlError::WrongConnectionKind);
    }
    Ok(Command::ReconnectClient { mac: client.mac().to_string() })
}

/// Set the `PoE` mode of a switch port.
///
/// Validates that the device is a switch, the port exists, and the port can
/// actually deliver power.
///
/// # Errors
/// - [`ControlError::NotASwitch`] if `device` is not a switch.
/// - [`ControlError::UnknownPort`] if `port` is not present on the device.
/// - [`ControlError::PortNotPoeCapable`] if the port cannot deliver `PoE`.
pub fn set_poe(
    device: &NetworkDevice,
    port: u16,
    mode: PoeMode,
) -> Result<Command, ControlError> {
    if device.kind() != DeviceKind::Switch {
        return Err(ControlError::NotASwitch);
    }
    let Some(p) = device.port(port) else {
        return Err(ControlError::UnknownPort(port));
    };
    if !p.poe_capable {
        return Err(ControlError::PortNotPoeCapable(port));
    }
    Ok(Command::SetPoe { device_id: device.id().to_string(), port, mode })
}

/// Enable or disable a wireless network by name.
#[must_use]
pub fn set_wlan_enabled(wlan: impl Into<String>, enabled: bool) -> Command {
    Command::SetWlanEnabled { wlan: wlan.into(), enabled }
}

/// Enable or disable a port-forward rule by name.
#[must_use]
pub fn set_port_forward_enabled(rule: impl Into<String>, enabled: bool) -> Command {
    Command::SetPortForwardEnabled { rule: rule.into(), enabled }
}

/// Turn a device's status LED on or off.
#[must_use]
pub fn set_device_led(device: &NetworkDevice, on: bool) -> Command {
    Command::SetDeviceLed { device_id: device.id().to_string(), on }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::SwitchPort;

    fn switch_with_ports() -> NetworkDevice {
        NetworkDevice::new("sw-1", "Switch", "aa:00", DeviceKind::Switch).with_ports(vec![
            SwitchPort::new(1, true),
            SwitchPort::new(2, false),
        ])
    }

    #[test]
    fn block_then_double_block_is_rejected() {
        let c = NetworkClient::new("aa:bb", "Tablet").wireless("Home", "ap-1");
        assert_eq!(
            block_client(&c).unwrap(),
            Command::BlockClient { mac: "aa:bb".to_string() }
        );
        let blocked = c.blocked();
        assert_eq!(block_client(&blocked), Err(ControlError::AlreadyInState));
    }

    #[test]
    fn unblock_requires_blocked_client() {
        let c = NetworkClient::new("aa:bb", "Tablet");
        assert_eq!(unblock_client(&c), Err(ControlError::AlreadyInState));
        let blocked = c.blocked();
        assert_eq!(
            unblock_client(&blocked).unwrap(),
            Command::UnblockClient { mac: "aa:bb".to_string() }
        );
    }

    #[test]
    fn reconnect_only_for_wireless() {
        let wired = NetworkClient::new("aa:bb", "Desktop").wired_to("sw-1");
        assert_eq!(reconnect_client(&wired), Err(ControlError::WrongConnectionKind));
        let wifi = NetworkClient::new("aa:bb", "Laptop").wireless("Home", "ap-1");
        assert_eq!(
            reconnect_client(&wifi).unwrap(),
            Command::ReconnectClient { mac: "aa:bb".to_string() }
        );
    }

    #[test]
    fn set_poe_on_valid_port() {
        let sw = switch_with_ports();
        assert_eq!(
            set_poe(&sw, 1, PoeMode::On).unwrap(),
            Command::SetPoe { device_id: "sw-1".to_string(), port: 1, mode: PoeMode::On }
        );
    }

    #[test]
    fn set_poe_rejects_unknown_port() {
        let sw = switch_with_ports();
        assert_eq!(set_poe(&sw, 99, PoeMode::Auto), Err(ControlError::UnknownPort(99)));
    }

    #[test]
    fn set_poe_rejects_non_poe_port() {
        let sw = switch_with_ports();
        assert_eq!(set_poe(&sw, 2, PoeMode::On), Err(ControlError::PortNotPoeCapable(2)));
    }

    #[test]
    fn set_poe_rejects_non_switch() {
        let ap = NetworkDevice::new("ap-1", "AP", "bb:11", DeviceKind::AccessPoint)
            .with_ports(vec![SwitchPort::new(1, true)]);
        assert_eq!(set_poe(&ap, 1, PoeMode::On), Err(ControlError::NotASwitch));
    }

    #[test]
    fn wlan_and_port_forward_toggles() {
        assert_eq!(
            set_wlan_enabled("Guest", false),
            Command::SetWlanEnabled { wlan: "Guest".to_string(), enabled: false }
        );
        assert_eq!(
            set_port_forward_enabled("Web server", true),
            Command::SetPortForwardEnabled { rule: "Web server".to_string(), enabled: true }
        );
    }

    #[test]
    fn device_led_toggle() {
        let sw = switch_with_ports();
        assert_eq!(
            set_device_led(&sw, false),
            Command::SetDeviceLed { device_id: "sw-1".to_string(), on: false }
        );
    }

    #[test]
    fn poe_mode_wire_strings() {
        assert_eq!(PoeMode::On.as_wire(), "on");
        assert_eq!(PoeMode::Off.as_wire(), "off");
        assert_eq!(PoeMode::Auto.as_wire(), "auto");
    }
}
