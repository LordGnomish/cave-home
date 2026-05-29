//! Network configuration model — Wi-Fi networks, firewall port-forwards,
//! the guest network, bandwidth profiles, and VLANs / subnets.
//!
//! These are the typed configuration objects a household reasons about. They
//! are deliberately plain data with light validation; the control engine
//! ([`crate::control`]) toggles them and the labels module
//! ([`crate::label`]) describes them in household words.

use std::net::IpAddr;

/// A wireless network (SSID) the household can switch on or off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wlan {
    pub name: String,
    pub ssid: String,
    pub enabled: bool,
    /// Whether this WLAN is the isolated guest network.
    pub is_guest: bool,
}

impl Wlan {
    #[must_use]
    pub fn new(name: impl Into<String>, ssid: impl Into<String>) -> Self {
        Self { name: name.into(), ssid: ssid.into(), enabled: true, is_guest: false }
    }

    #[must_use]
    pub fn guest(mut self) -> Self {
        self.is_guest = true;
        self
    }

    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// The transport protocol a port-forward rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Both,
}

/// Why a [`PortForward`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkError {
    /// A port number was 0 (not a usable port).
    InvalidPort,
}

impl core::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidPort => f.write_str("port number must be between 1 and 65535"),
        }
    }
}

impl std::error::Error for NetworkError {}

/// A firewall port-forward rule (forwards an outside port to an inside host).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortForward {
    pub name: String,
    pub protocol: Protocol,
    forward_port: u16,
    dest_ip: IpAddr,
    dest_port: u16,
    pub enabled: bool,
}

impl PortForward {
    /// Construct a port-forward rule.
    ///
    /// # Errors
    /// [`NetworkError::InvalidPort`] if either port is 0.
    pub fn new(
        name: impl Into<String>,
        protocol: Protocol,
        forward_port: u16,
        dest_ip: IpAddr,
        dest_port: u16,
    ) -> Result<Self, NetworkError> {
        if forward_port == 0 || dest_port == 0 {
            return Err(NetworkError::InvalidPort);
        }
        Ok(Self {
            name: name.into(),
            protocol,
            forward_port,
            dest_ip,
            dest_port,
            enabled: true,
        })
    }

    #[must_use]
    pub const fn forward_port(&self) -> u16 {
        self.forward_port
    }

    #[must_use]
    pub const fn dest_ip(&self) -> IpAddr {
        self.dest_ip
    }

    #[must_use]
    pub const fn dest_port(&self) -> u16 {
        self.dest_port
    }
}

/// A reusable bandwidth limit (download / upload caps in kbit/s; `None` = no
/// cap on that direction). Used to throttle the guest network or a kid's
/// device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandwidthProfile {
    pub download_kbps: Option<u32>,
    pub upload_kbps: Option<u32>,
}

impl BandwidthProfile {
    /// An unlimited profile (no caps either way).
    #[must_use]
    pub const fn unlimited() -> Self {
        Self { download_kbps: None, upload_kbps: None }
    }

    /// A symmetric cap applied to both directions.
    #[must_use]
    pub const fn capped(kbps: u32) -> Self {
        Self { download_kbps: Some(kbps), upload_kbps: Some(kbps) }
    }

    #[must_use]
    pub const fn is_unlimited(&self) -> bool {
        self.download_kbps.is_none() && self.upload_kbps.is_none()
    }
}

/// The guest network: an isolated WLAN with its own bandwidth cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestNetwork {
    pub wlan: Wlan,
    pub bandwidth: BandwidthProfile,
}

impl GuestNetwork {
    /// A guest network on `ssid`, enabled, with the given bandwidth cap.
    #[must_use]
    pub fn new(ssid: impl Into<String>, bandwidth: BandwidthProfile) -> Self {
        let s = ssid.into();
        Self { wlan: Wlan::new(s.clone(), s).guest(), bandwidth }
    }

    #[must_use]
    pub const fn is_on(&self) -> bool {
        self.wlan.enabled
    }
}

/// What a VLAN / network segment is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPurpose {
    /// The main household ("corporate" in `UniFi` terms) network.
    Corporate,
    /// The isolated guest network.
    Guest,
    /// A segment for smart-home / `IoT` devices.
    Iot,
}

impl NetworkPurpose {
    /// Plain-language purpose for the household (Charter §6.3).
    #[must_use]
    pub const fn household_word(self) -> &'static str {
        match self {
            Self::Corporate => "main network",
            Self::Guest => "guest network",
            Self::Iot => "smart-home devices",
        }
    }
}

/// A VLAN / network segment with its subnet.
///
/// The subnet is modelled as a base [`IpAddr`] plus a CIDR prefix length,
/// validated on construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vlan {
    pub id: u16,
    pub name: String,
    subnet: IpAddr,
    prefix_len: u8,
    pub purpose: NetworkPurpose,
}

/// Why a [`Vlan`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VlanError {
    /// VLAN id outside the 802.1Q-usable 1..=4094 range.
    InvalidId,
    /// CIDR prefix too long for the address family.
    InvalidPrefix,
}

impl core::fmt::Display for VlanError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidId => f.write_str("network id must be between 1 and 4094"),
            Self::InvalidPrefix => f.write_str("subnet prefix length is too long"),
        }
    }
}

impl std::error::Error for VlanError {}

impl Vlan {
    /// Construct a VLAN / network segment.
    ///
    /// # Errors
    /// - [`VlanError::InvalidId`] if `id` is not in 1..=4094.
    /// - [`VlanError::InvalidPrefix`] if `prefix_len` exceeds 32 (IPv4) or
    ///   128 (IPv6).
    pub fn new(
        id: u16,
        name: impl Into<String>,
        subnet: IpAddr,
        prefix_len: u8,
        purpose: NetworkPurpose,
    ) -> Result<Self, VlanError> {
        if id == 0 || id > 4094 {
            return Err(VlanError::InvalidId);
        }
        let max = if subnet.is_ipv4() { 32 } else { 128 };
        if prefix_len > max {
            return Err(VlanError::InvalidPrefix);
        }
        Ok(Self { id, name: name.into(), subnet, prefix_len, purpose })
    }

    #[must_use]
    pub const fn subnet(&self) -> IpAddr {
        self.subnet
    }

    #[must_use]
    pub const fn prefix_len(&self) -> u8 {
        self.prefix_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn wlan_defaults_enabled_not_guest() {
        let w = Wlan::new("Home", "Home-5G");
        assert!(w.enabled);
        assert!(!w.is_guest);
        let g = Wlan::new("Guest", "Guest").guest().disabled();
        assert!(g.is_guest);
        assert!(!g.enabled);
    }

    #[test]
    fn port_forward_rejects_zero_port() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        assert_eq!(
            PortForward::new("bad", Protocol::Tcp, 0, ip, 80),
            Err(NetworkError::InvalidPort)
        );
        assert_eq!(
            PortForward::new("bad", Protocol::Tcp, 80, ip, 0),
            Err(NetworkError::InvalidPort)
        );
    }

    #[test]
    fn port_forward_constructs_and_is_enabled() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let pf = PortForward::new("Web", Protocol::Both, 443, ip, 8443).unwrap();
        assert_eq!(pf.forward_port(), 443);
        assert_eq!(pf.dest_port(), 8443);
        assert_eq!(pf.dest_ip(), ip);
        assert!(pf.enabled);
    }

    #[test]
    fn bandwidth_profile_unlimited_and_capped() {
        assert!(BandwidthProfile::unlimited().is_unlimited());
        let c = BandwidthProfile::capped(50_000);
        assert!(!c.is_unlimited());
        assert_eq!(c.download_kbps, Some(50_000));
        assert_eq!(c.upload_kbps, Some(50_000));
    }

    #[test]
    fn guest_network_is_isolated_wlan() {
        let g = GuestNetwork::new("Visitors", BandwidthProfile::capped(20_000));
        assert!(g.wlan.is_guest);
        assert!(g.is_on());
    }

    #[test]
    fn vlan_rejects_bad_id() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 30, 0));
        assert_eq!(
            Vlan::new(0, "x", ip, 24, NetworkPurpose::Iot),
            Err(VlanError::InvalidId)
        );
        assert_eq!(
            Vlan::new(5000, "x", ip, 24, NetworkPurpose::Iot),
            Err(VlanError::InvalidId)
        );
    }

    #[test]
    fn vlan_rejects_oversize_prefix() {
        let v4 = IpAddr::V4(Ipv4Addr::new(10, 0, 30, 0));
        assert_eq!(
            Vlan::new(30, "iot", v4, 33, NetworkPurpose::Iot),
            Err(VlanError::InvalidPrefix)
        );
        // 33 is fine for v6.
        let v6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
        assert!(Vlan::new(30, "iot", v6, 64, NetworkPurpose::Iot).is_ok());
    }

    #[test]
    fn vlan_constructs_with_subnet() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 30, 0));
        let v = Vlan::new(30, "IoT", ip, 24, NetworkPurpose::Iot).unwrap();
        assert_eq!(v.id, 30);
        assert_eq!(v.subnet(), ip);
        assert_eq!(v.prefix_len(), 24);
        assert_eq!(v.purpose, NetworkPurpose::Iot);
    }

    #[test]
    fn network_purpose_words_are_plain() {
        assert_eq!(NetworkPurpose::Corporate.household_word(), "main network");
        assert_eq!(NetworkPurpose::Guest.household_word(), "guest network");
        assert_eq!(NetworkPurpose::Iot.household_word(), "smart-home devices");
    }
}
