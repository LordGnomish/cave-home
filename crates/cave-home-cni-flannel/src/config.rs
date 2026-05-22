// SPDX-License-Identifier: Apache-2.0
//! Network-wide flannel configuration.
//!
//! Upstream parity: `pkg/subnet/config.go` — the JSON blob stored at
//! `/coreos.com/network/config` in etcd (or its in-memory equivalent for
//! single-node MemRegistry).
//!
//! Phase 1 only models IPv4. IPv6 single-stack and dual-stack are deferred to
//! Phase 1b (see `parity.manifest.toml` [[unmapped]]).

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};

/// Wire-format network config (matches the JSON in etcd / mem registry).
///
/// Upstream `Config` struct: `pkg/subnet/config.go` lines 30..52.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkConfig {
    /// Cluster-wide CIDR (e.g. `10.244.0.0/16`).
    #[serde(rename = "Network")]
    pub network: Ipv4Net,

    /// Per-node subnet length (e.g. `24` carves /16 into 256 /24 leases).
    #[serde(rename = "SubnetLen", default = "default_subnet_len")]
    pub subnet_len: u8,

    /// Lower bound of the subnet allocation range. `None` → derived from
    /// `network` (first usable subnet).
    #[serde(rename = "SubnetMin", default, skip_serializing_if = "Option::is_none")]
    pub subnet_min: Option<Ipv4Net>,

    /// Upper bound of the subnet allocation range. `None` → last subnet.
    #[serde(rename = "SubnetMax", default, skip_serializing_if = "Option::is_none")]
    pub subnet_max: Option<Ipv4Net>,

    /// IPv4 enabled flag. Defaults to true.
    #[serde(rename = "EnableIPv4", default = "default_true")]
    pub enable_ipv4: bool,

    /// IPv6 enabled flag. Phase 1 keeps this off.
    #[serde(rename = "EnableIPv6", default)]
    pub enable_ipv6: bool,

    /// Selected backend.
    #[serde(rename = "Backend", default)]
    pub backend: BackendConfig,
}

const fn default_subnet_len() -> u8 {
    24
}

const fn default_true() -> bool {
    true
}

/// Backend-type selector (subset of upstream supported backends).
///
/// Upstream defines `vxlan`, `host-gw`, `wireguard`, `ipsec`, `udp`, `alloc`,
/// `extension`. Phase 1 ships VXLAN only — others are tracked in
/// `parity.manifest.toml` [[unmapped]] with priority `phase-1b`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "Type")]
pub enum BackendConfig {
    /// VXLAN datapath (overlay; default).
    #[serde(rename = "vxlan")]
    Vxlan(VxlanBackendConfig),
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self::Vxlan(VxlanBackendConfig::default())
    }
}

/// VXLAN-specific knobs.
///
/// Upstream parity: `pkg/backend/vxlan/vxlan.go` lines 50..80 (the
/// `VXLANBackendConfig` struct).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VxlanBackendConfig {
    /// VXLAN Network Identifier.
    #[serde(rename = "VNI", default = "default_vni")]
    pub vni: u32,

    /// UDP port (IANA assignment for VXLAN is 4789; flannel historically used
    /// 8472 to match Linux kernel default).
    #[serde(rename = "Port", default = "default_port")]
    pub port: u16,

    /// Group-Based Policy extension (deferred — Phase 1b).
    #[serde(rename = "GBP", default)]
    pub gbp: bool,

    /// Optional override MAC address for the `flannel.<vni>` device.
    #[serde(rename = "MacPrefix", default, skip_serializing_if = "Option::is_none")]
    pub mac_prefix: Option<String>,
}

impl Default for VxlanBackendConfig {
    fn default() -> Self {
        Self {
            vni: default_vni(),
            port: default_port(),
            gbp: false,
            mac_prefix: None,
        }
    }
}

const fn default_vni() -> u32 {
    1
}

const fn default_port() -> u16 {
    8472
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_minimal_config() {
        let raw = r#"{"Network":"10.244.0.0/16"}"#;
        let cfg: NetworkConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.network.to_string(), "10.244.0.0/16");
        assert_eq!(cfg.subnet_len, 24);
        assert!(cfg.enable_ipv4);
        assert!(!cfg.enable_ipv6);
        assert!(matches!(cfg.backend, BackendConfig::Vxlan(_)));
    }

    #[test]
    fn deserializes_full_vxlan_config() {
        let raw = r#"{
            "Network":"10.42.0.0/16",
            "SubnetLen":24,
            "EnableIPv4":true,
            "Backend":{"Type":"vxlan","VNI":4096,"Port":4789}
        }"#;
        let cfg: NetworkConfig = serde_json::from_str(raw).unwrap();
        let BackendConfig::Vxlan(v) = &cfg.backend;
        assert_eq!(v.vni, 4096);
        assert_eq!(v.port, 4789);
    }

    #[test]
    fn vxlan_default_port_is_8472() {
        let v = VxlanBackendConfig::default();
        assert_eq!(v.port, 8472);
        assert_eq!(v.vni, 1);
    }
}
