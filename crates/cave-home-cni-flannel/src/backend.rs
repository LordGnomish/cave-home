// SPDX-License-Identifier: Apache-2.0
//! flannel backend configuration model.
//!
//! flannel supports several datapath *backends* selected in the cluster
//! network config. This module models the typed configuration for the three
//! the cave-home roadmap cares about — VXLAN (the default), host-gw, and
//! `WireGuard` — plus the per-node *backend data* each node advertises so its
//! peers can reach it (for VXLAN: the node's VTEP MAC and its public/underlay
//! IP).
//!
//! This is configuration and per-node-attribute modelling only. Bringing up
//! the actual VXLAN device, programming the kernel FDB, or establishing
//! `WireGuard` tunnels is the deferred netlink/datapath layer (see the parity
//! manifest). The route-list this config implies is computed in
//! [`crate::routes`].

use std::fmt;
use std::net::IpAddr;

/// A 48-bit Ethernet (MAC) address — the VTEP MAC a node advertises for VXLAN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MacAddr([u8; 6]);

/// Error parsing a MAC address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacParseError(String);

impl fmt::Display for MacParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid MAC address '{}'", self.0)
    }
}

impl std::error::Error for MacParseError {}

impl MacAddr {
    /// Construct from raw octets.
    #[must_use]
    pub const fn new(octets: [u8; 6]) -> Self {
        Self(octets)
    }

    /// The raw octets.
    #[must_use]
    pub const fn octets(&self) -> [u8; 6] {
        self.0
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let o = self.0;
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            o[0], o[1], o[2], o[3], o[4], o[5]
        )
    }
}

impl std::str::FromStr for MacAddr {
    type Err = MacParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut octets = [0u8; 6];
        let mut count = 0usize;
        for part in s.split(':') {
            if count >= 6 {
                return Err(MacParseError(s.to_owned()));
            }
            let byte =
                u8::from_str_radix(part, 16).map_err(|_| MacParseError(s.to_owned()))?;
            // Reject "1" vs "01" ambiguity-free: require exactly 1-2 hex chars.
            if part.is_empty() || part.len() > 2 {
                return Err(MacParseError(s.to_owned()));
            }
            octets[count] = byte;
            count += 1;
        }
        if count != 6 {
            return Err(MacParseError(s.to_owned()));
        }
        Ok(Self(octets))
    }
}

/// The default VXLAN UDP port flannel uses (the Linux kernel VXLAN default).
pub const DEFAULT_VXLAN_PORT: u16 = 8472;
/// The default VXLAN Network Identifier (VNI) flannel uses.
pub const DEFAULT_VNI: u32 = 1;

/// Typed flannel backend configuration (the `Backend` block of the network
/// config).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendConfig {
    /// VXLAN overlay (flannel default).
    Vxlan(VxlanConfig),
    /// Direct routing — peers reachable on the same L2, no encapsulation.
    HostGw,
    /// Encrypted overlay over `WireGuard`.
    Wireguard(WireguardConfig),
}

impl BackendConfig {
    /// The backend type name as flannel writes it in the network config JSON.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Vxlan(_) => "vxlan",
            Self::HostGw => "host-gw",
            Self::Wireguard(_) => "wireguard",
        }
    }

    /// `true` if this backend encapsulates traffic (VXLAN, `WireGuard`) rather
    /// than routing it directly (host-gw). Encapsulating backends reduce the
    /// usable MTU.
    #[must_use]
    pub const fn is_encapsulating(&self) -> bool {
        matches!(self, Self::Vxlan(_) | Self::Wireguard(_))
    }
}

/// VXLAN backend configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlanConfig {
    /// VXLAN Network Identifier.
    pub vni: u32,
    /// UDP port for VXLAN traffic.
    pub port: u16,
    /// Whether to enable directRouting (use a direct route for same-subnet
    /// peers instead of encapsulating). flannel's `DirectRouting` option.
    pub direct_routing: bool,
}

impl Default for VxlanConfig {
    fn default() -> Self {
        Self {
            vni: DEFAULT_VNI,
            port: DEFAULT_VXLAN_PORT,
            direct_routing: false,
        }
    }
}

impl VxlanConfig {
    /// The encapsulation overhead in bytes VXLAN adds to each packet
    /// (outer Ethernet 14 + outer IPv4 20 + UDP 8 + VXLAN header 8 = 50).
    /// flannel subtracts this from the link MTU to compute the overlay MTU.
    pub const ENCAP_OVERHEAD: u32 = 50;

    /// Compute the overlay MTU for a given underlay link MTU.
    #[must_use]
    pub const fn overlay_mtu(&self, link_mtu: u32) -> u32 {
        link_mtu.saturating_sub(Self::ENCAP_OVERHEAD)
    }
}

/// `WireGuard` backend configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireguardConfig {
    /// UDP listen port for the `WireGuard` tunnel.
    pub port: u16,
    /// Persistent-keepalive interval in seconds (0 = disabled).
    pub keepalive_secs: u16,
}

impl WireguardConfig {
    /// `WireGuard` encapsulation overhead in bytes (outer IPv4 20 + UDP 8 +
    /// `WireGuard` data header 32 = 60). Used to derive the overlay MTU.
    pub const ENCAP_OVERHEAD: u32 = 60;

    /// Compute the overlay MTU for a given underlay link MTU.
    #[must_use]
    pub const fn overlay_mtu(&self, link_mtu: u32) -> u32 {
        link_mtu.saturating_sub(Self::ENCAP_OVERHEAD)
    }
}

impl Default for WireguardConfig {
    fn default() -> Self {
        Self {
            port: 51_820,
            keepalive_secs: 0,
        }
    }
}

/// The per-node backend attributes a node advertises so peers can reach it.
///
/// flannel stores this as the lease's `BackendData` / `BackendType` /
/// `PublicIP` attributes. The shape depends on the backend: VXLAN peers need
/// the VTEP MAC and the underlay public IP; host-gw peers only need the
/// next-hop (public) IP; `WireGuard` peers need the public key and endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeBackendData {
    /// VXLAN VTEP attributes.
    Vxlan {
        /// The node's underlay / public IP (the VXLAN tunnel endpoint).
        public_ip: IpAddr,
        /// The MAC of the node's `flannel.<vni>` VTEP device.
        vtep_mac: MacAddr,
    },
    /// host-gw next hop.
    HostGw {
        /// The node's IP on the shared L2 — used as the route next-hop.
        public_ip: IpAddr,
    },
    /// `WireGuard` peer attributes.
    Wireguard {
        /// The node's `WireGuard` endpoint IP.
        public_ip: IpAddr,
        /// The node's base64 `WireGuard` public key.
        public_key: String,
    },
}

impl NodeBackendData {
    /// The node's underlay/public IP, common to every backend.
    #[must_use]
    pub const fn public_ip(&self) -> IpAddr {
        match self {
            Self::Vxlan { public_ip, .. }
            | Self::HostGw { public_ip }
            | Self::Wireguard { public_ip, .. } => *public_ip,
        }
    }

    /// `true` if this per-node data matches the cluster backend type.
    #[must_use]
    pub const fn matches(&self, cfg: &BackendConfig) -> bool {
        matches!(
            (self, cfg),
            (Self::Vxlan { .. }, BackendConfig::Vxlan(_))
                | (Self::HostGw { .. }, BackendConfig::HostGw)
                | (Self::Wireguard { .. }, BackendConfig::Wireguard(_))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }

    #[test]
    fn mac_round_trips() {
        let m = MacAddr::from_str("0a:1b:2c:3d:4e:5f").expect("mac");
        assert_eq!(m.octets(), [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f]);
        assert_eq!(m.to_string(), "0a:1b:2c:3d:4e:5f");
    }

    #[test]
    fn mac_round_trips_uppercase_to_lowercase() {
        let m = MacAddr::from_str("AA:BB:CC:DD:EE:FF").expect("mac");
        assert_eq!(m.to_string(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn mac_rejects_too_few_octets() {
        assert!(MacAddr::from_str("0a:1b:2c").is_err());
    }

    #[test]
    fn mac_rejects_too_many_octets() {
        assert!(MacAddr::from_str("0a:1b:2c:3d:4e:5f:60").is_err());
    }

    #[test]
    fn mac_rejects_non_hex() {
        assert!(MacAddr::from_str("zz:1b:2c:3d:4e:5f").is_err());
    }

    #[test]
    fn mac_rejects_overlong_octet() {
        assert!(MacAddr::from_str("0a0:1b:2c:3d:4e:5f").is_err());
    }

    #[test]
    fn vxlan_defaults_match_flannel() {
        let c = VxlanConfig::default();
        assert_eq!(c.vni, 1);
        assert_eq!(c.port, 8472);
        assert!(!c.direct_routing);
    }

    #[test]
    fn vxlan_overlay_mtu_subtracts_overhead() {
        let c = VxlanConfig::default();
        assert_eq!(c.overlay_mtu(1500), 1450);
        // saturates rather than underflowing on a tiny link MTU.
        assert_eq!(c.overlay_mtu(10), 0);
    }

    #[test]
    fn wireguard_overlay_mtu_subtracts_overhead() {
        let c = WireguardConfig::default();
        assert_eq!(c.overlay_mtu(1500), 1440);
        assert_eq!(c.port, 51_820);
    }

    #[test]
    fn backend_type_names_match_flannel() {
        assert_eq!(BackendConfig::Vxlan(VxlanConfig::default()).type_name(), "vxlan");
        assert_eq!(BackendConfig::HostGw.type_name(), "host-gw");
        assert_eq!(
            BackendConfig::Wireguard(WireguardConfig::default()).type_name(),
            "wireguard"
        );
    }

    #[test]
    fn encapsulation_classification() {
        assert!(BackendConfig::Vxlan(VxlanConfig::default()).is_encapsulating());
        assert!(BackendConfig::Wireguard(WireguardConfig::default()).is_encapsulating());
        assert!(!BackendConfig::HostGw.is_encapsulating());
    }

    #[test]
    fn node_data_public_ip_is_common() {
        let mac = MacAddr::new([1, 2, 3, 4, 5, 6]);
        let d = NodeBackendData::Vxlan {
            public_ip: v4("192.168.1.10"),
            vtep_mac: mac,
        };
        assert_eq!(d.public_ip(), v4("192.168.1.10"));
    }

    #[test]
    fn node_data_matches_backend_type() {
        let vx = NodeBackendData::Vxlan {
            public_ip: v4("192.168.1.10"),
            vtep_mac: MacAddr::new([1, 2, 3, 4, 5, 6]),
        };
        assert!(vx.matches(&BackendConfig::Vxlan(VxlanConfig::default())));
        assert!(!vx.matches(&BackendConfig::HostGw));

        let hg = NodeBackendData::HostGw {
            public_ip: v4("192.168.1.11"),
        };
        assert!(hg.matches(&BackendConfig::HostGw));
        assert!(!hg.matches(&BackendConfig::Vxlan(VxlanConfig::default())));

        let wg = NodeBackendData::Wireguard {
            public_ip: v4("192.168.1.12"),
            public_key: "abc=".to_owned(),
        };
        assert!(wg.matches(&BackendConfig::Wireguard(WireguardConfig::default())));
    }

    #[test]
    fn custom_vxlan_vni_and_port() {
        let c = VxlanConfig {
            vni: 4096,
            port: 4789,
            direct_routing: true,
        };
        let b = BackendConfig::Vxlan(c.clone());
        assert_eq!(b.type_name(), "vxlan");
        assert!(c.direct_routing);
    }
}
