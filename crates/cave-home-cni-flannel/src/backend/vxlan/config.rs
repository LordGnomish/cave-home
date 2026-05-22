// SPDX-License-Identifier: Apache-2.0
//! VXLAN per-device parameters resolved from `NetworkConfig + VxlanBackendConfig`.
//!
//! Upstream parity: `pkg/backend/vxlan/vxlan.go::newVXLANDevice` argument struct.

use crate::config::VxlanBackendConfig;
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// Resolved VXLAN device attributes, ready to hand to the netlink layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlanDeviceAttrs {
    /// Linux device name (e.g. `flannel.1`).
    pub name: String,
    pub vni: u32,
    pub port: u16,
    /// Tunnel-endpoint local IP (this node's PublicIP).
    pub local_ip: Ipv4Addr,
    /// /32 address assigned to the device (typically the lowest usable in the
    /// node's lease).
    pub addr: Ipv4Net,
    /// Effective MTU: underlay MTU minus VXLAN/UDP/IP overhead (50 bytes).
    pub mtu: u32,
    /// Group-Based Policy on the device.
    pub gbp: bool,
    /// Pre-derived MAC; if `None` the kernel picks one.
    pub mac: Option<[u8; 6]>,
}

/// VTEP MAC payload stashed in `LeaseAttrs.backend_data` so peers can install
/// the FDB entry pointing remote subnets at this node's tunnel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VtepBackendData {
    #[serde(rename = "VtepMAC")]
    pub vtep_mac: String,
}

impl VxlanDeviceAttrs {
    /// Build attrs from the network config.
    ///
    /// Upstream parity: `pkg/backend/vxlan/vxlan.go::newSubnetAttrs` plus
    /// `RegisterNetwork`.
    #[must_use]
    pub fn from_config(
        cfg: &VxlanBackendConfig,
        local_ip: Ipv4Addr,
        local_subnet: Ipv4Net,
        underlay_mtu: u32,
    ) -> Self {
        Self {
            name: format!("flannel.{}", cfg.vni),
            vni: cfg.vni,
            port: cfg.port,
            local_ip,
            // The /32 inside the lease — first usable host address.
            addr: Ipv4Net::new(local_subnet.network(), 32).unwrap_or(local_subnet),
            mtu: underlay_mtu.saturating_sub(VXLAN_OVERHEAD_BYTES),
            gbp: cfg.gbp,
            mac: None,
        }
    }
}

/// IP(20) + UDP(8) + VXLAN(8) + Ethernet(14) = 50 bytes.
pub const VXLAN_OVERHEAD_BYTES: u32 = 50;
