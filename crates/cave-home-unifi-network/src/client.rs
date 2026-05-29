//! Network-client model — the phones, tablets, laptops and TVs on the network.
//!
//! A [`NetworkClient`] is what a household actually thinks of as "a device on
//! the Wi-Fi". The presence model ([`crate::presence`]) tracks one of these to
//! decide whether a family member is home; the control engine
//! ([`crate::control`]) blocks / unblocks / reconnects one.

use std::net::IpAddr;

/// How a client is attached to the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionKind {
    /// Plugged into a switch port with a cable.
    Wired,
    /// On Wi-Fi: carries the SSID it joined and the id of the AP serving it.
    Wireless { ssid: String, access_point: String },
}

impl ConnectionKind {
    #[must_use]
    pub const fn is_wireless(&self) -> bool {
        matches!(self, Self::Wireless { .. })
    }

    /// The SSID, when this is a wireless connection.
    #[must_use]
    pub fn ssid(&self) -> Option<&str> {
        match self {
            Self::Wireless { ssid, .. } => Some(ssid),
            Self::Wired => None,
        }
    }

    /// The id of the access point serving this client, when wireless.
    #[must_use]
    pub fn access_point(&self) -> Option<&str> {
        match self {
            Self::Wireless { access_point, .. } => Some(access_point),
            Self::Wired => None,
        }
    }
}

/// A client on the network.
///
/// Defaults: a freshly-constructed client is wired, has no IP, is not a guest,
/// is not blocked, and was last seen at tick 0. The builder methods set the
/// rest. `last_seen` is a caller-supplied monotonic tick (seconds since some
/// epoch the caller chooses); the crate never reads a clock itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkClient {
    mac: String,
    name: String,
    ip: Option<IpAddr>,
    connection: ConnectionKind,
    uplink_device: Option<String>,
    is_guest: bool,
    is_blocked: bool,
    last_seen: u64,
}

impl NetworkClient {
    /// Construct a client by MAC and friendly name. Wired, not a guest, not
    /// blocked, last seen at tick 0 until the builders say otherwise.
    #[must_use]
    pub fn new(mac: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            mac: mac.into(),
            name: name.into(),
            ip: None,
            connection: ConnectionKind::Wired,
            uplink_device: None,
            is_guest: false,
            is_blocked: false,
            last_seen: 0,
        }
    }

    /// Builder: mark this client wireless on `ssid`, served by `access_point`.
    #[must_use]
    pub fn wireless(mut self, ssid: impl Into<String>, access_point: impl Into<String>) -> Self {
        let ap = access_point.into();
        self.uplink_device = Some(ap.clone());
        self.connection = ConnectionKind::Wireless { ssid: ssid.into(), access_point: ap };
        self
    }

    /// Builder: mark this client wired through switch/device `device_id`.
    #[must_use]
    pub fn wired_to(mut self, device_id: impl Into<String>) -> Self {
        self.uplink_device = Some(device_id.into());
        self.connection = ConnectionKind::Wired;
        self
    }

    /// Builder: set the client's IP address.
    #[must_use]
    pub fn with_ip(mut self, ip: IpAddr) -> Self {
        self.ip = Some(ip);
        self
    }

    /// Builder: record the last-seen tick (caller's monotonic clock).
    #[must_use]
    pub fn last_seen_at(mut self, tick: u64) -> Self {
        self.last_seen = tick;
        self
    }

    /// Builder: mark this client as a guest-network client.
    #[must_use]
    pub fn as_guest(mut self) -> Self {
        self.is_guest = true;
        self
    }

    /// Builder: mark this client as currently blocked.
    #[must_use]
    pub fn blocked(mut self) -> Self {
        self.is_blocked = true;
        self
    }

    #[must_use]
    pub fn mac(&self) -> &str {
        &self.mac
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn ip(&self) -> Option<IpAddr> {
        self.ip
    }

    #[must_use]
    pub const fn connection(&self) -> &ConnectionKind {
        &self.connection
    }

    /// The id of the device (AP or switch) this client connects through.
    #[must_use]
    pub fn uplink_device(&self) -> Option<&str> {
        self.uplink_device.as_deref()
    }

    #[must_use]
    pub const fn is_guest(&self) -> bool {
        self.is_guest
    }

    #[must_use]
    pub const fn is_blocked(&self) -> bool {
        self.is_blocked
    }

    #[must_use]
    pub const fn last_seen(&self) -> u64 {
        self.last_seen
    }

    #[must_use]
    pub const fn is_wireless(&self) -> bool {
        self.connection.is_wireless()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn new_client_defaults_are_safe() {
        let c = NetworkClient::new("aa:bb", "Phone");
        assert_eq!(c.mac(), "aa:bb");
        assert_eq!(c.name(), "Phone");
        assert_eq!(c.ip(), None);
        assert!(!c.is_wireless());
        assert!(!c.is_guest());
        assert!(!c.is_blocked());
        assert_eq!(c.last_seen(), 0);
        assert_eq!(c.uplink_device(), None);
    }

    #[test]
    fn wireless_builder_sets_ssid_ap_and_uplink() {
        let c = NetworkClient::new("aa:bb", "Tablet").wireless("Home", "ap-1");
        assert!(c.is_wireless());
        assert_eq!(c.connection().ssid(), Some("Home"));
        assert_eq!(c.connection().access_point(), Some("ap-1"));
        assert_eq!(c.uplink_device(), Some("ap-1"));
    }

    #[test]
    fn wired_client_has_no_ssid() {
        let c = NetworkClient::new("aa:bb", "Desktop").wired_to("sw-1");
        assert!(!c.is_wireless());
        assert_eq!(c.connection().ssid(), None);
        assert_eq!(c.connection().access_point(), None);
        assert_eq!(c.uplink_device(), Some("sw-1"));
    }

    #[test]
    fn ip_uses_std_net() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50));
        let c = NetworkClient::new("aa:bb", "Laptop").with_ip(ip);
        assert_eq!(c.ip(), Some(ip));
    }

    #[test]
    fn guest_and_blocked_and_last_seen_builders() {
        let c = NetworkClient::new("aa:bb", "Visitor phone")
            .as_guest()
            .blocked()
            .last_seen_at(1234);
        assert!(c.is_guest());
        assert!(c.is_blocked());
        assert_eq!(c.last_seen(), 1234);
    }
}
