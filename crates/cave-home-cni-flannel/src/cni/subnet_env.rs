// SPDX-License-Identifier: Apache-2.0
//! `/run/flannel/subnet.env` parser.
//!
//! Upstream parity: `cni-plugin/flannel.go::loadFlannelSubnetEnv`. The file
//! looks like:
//!
//! ```text
//! FLANNEL_NETWORK=10.244.0.0/16
//! FLANNEL_SUBNET=10.244.1.1/24
//! FLANNEL_MTU=1450
//! FLANNEL_IPMASQ=true
//! ```

use crate::cni::handler::CniError;
use ipnet::Ipv4Net;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubnetEnv {
    pub network: Ipv4Net,
    pub subnet: Ipv4Net,
    pub mtu: u32,
    pub ip_masq: bool,
    /// IPv6 fields are deferred (Phase 1b).
    pub ip6_network: Option<ipnet::Ipv6Net>,
    pub ip6_subnet: Option<ipnet::Ipv6Net>,
}

/// Parse the contents of `subnet.env` (we take a string so callers can mock
/// the filesystem in tests).
pub fn parse_subnet_env(contents: &str) -> Result<SubnetEnv, CniError> {
    let mut network = None;
    let mut subnet = None;
    let mut mtu = None;
    let mut ip_masq = None;
    let mut ip6_network = None;
    let mut ip6_subnet = None;

    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(CniError::Parse(format!("malformed subnet.env line: {raw}")));
        };
        match key.trim() {
            "FLANNEL_NETWORK" => {
                network = Some(value.trim().parse::<Ipv4Net>().map_err(|e| {
                    CniError::Parse(format!("FLANNEL_NETWORK: {e}"))
                })?);
            }
            "FLANNEL_SUBNET" => {
                subnet = Some(value.trim().parse::<Ipv4Net>().map_err(|e| {
                    CniError::Parse(format!("FLANNEL_SUBNET: {e}"))
                })?);
            }
            "FLANNEL_MTU" => {
                mtu = Some(value.trim().parse::<u32>().map_err(|e| {
                    CniError::Parse(format!("FLANNEL_MTU: {e}"))
                })?);
            }
            "FLANNEL_IPMASQ" => {
                ip_masq = Some(matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1"));
            }
            "FLANNEL_IPV6_NETWORK" => {
                ip6_network = Some(value.trim().parse::<ipnet::Ipv6Net>().map_err(|e| {
                    CniError::Parse(format!("FLANNEL_IPV6_NETWORK: {e}"))
                })?);
            }
            "FLANNEL_IPV6_SUBNET" => {
                ip6_subnet = Some(value.trim().parse::<ipnet::Ipv6Net>().map_err(|e| {
                    CniError::Parse(format!("FLANNEL_IPV6_SUBNET: {e}"))
                })?);
            }
            _ => {} // forward-compatible: ignore unknown keys
        }
    }

    Ok(SubnetEnv {
        network: network.ok_or_else(|| CniError::Parse("missing FLANNEL_NETWORK".into()))?,
        subnet: subnet.ok_or_else(|| CniError::Parse("missing FLANNEL_SUBNET".into()))?,
        mtu: mtu.ok_or_else(|| CniError::Parse("missing FLANNEL_MTU".into()))?,
        ip_masq: ip_masq.unwrap_or(false),
        ip6_network,
        ip6_subnet,
    })
}
