// SPDX-License-Identifier: Apache-2.0
//! The `subnet.env` contract — port of `pkg/subnet/subnet.go::WriteSubnetFile`.
//!
//! `subnet.env` is the hand-off between the flannel daemon and the per-pod CNI
//! plugin. After the daemon leases this node its pod subnet it writes
//! `/run/flannel/subnet.env`; the `/opt/cni/bin/flannel` plugin reads it to
//! learn the network, the node subnet, the MTU and whether to masquerade, then
//! delegates to host-local IPAM + bridge. The file is four (or six, dual-stack)
//! `KEY=value` lines:
//!
//! ```text
//! FLANNEL_NETWORK=10.42.0.0/16
//! FLANNEL_SUBNET=10.42.1.1/24
//! FLANNEL_MTU=1450
//! FLANNEL_IPMASQ=false
//! ```
//!
//! Note the upstream subtlety this ports faithfully: `FLANNEL_SUBNET` is the
//! subnet's *first usable* address (`sn.IncrementIP()` → the `.1` gateway), not
//! the `.0` network address, carried at the subnet prefix length.

use std::fmt::Write as _;

use crate::cidr::Cidr;

/// The contents of a node's `subnet.env`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubnetEnv {
    /// The whole cluster pod network (`FLANNEL_NETWORK`).
    pub network: Cidr,
    /// This node's leased pod subnet (`FLANNEL_SUBNET`), rendered as the `.1`
    /// gateway over the subnet prefix.
    pub subnet: Cidr,
    /// The overlay MTU (`FLANNEL_MTU`).
    pub mtu: u32,
    /// Whether IP masquerade is enabled (`FLANNEL_IPMASQ`).
    pub ipmasq: bool,
}

impl SubnetEnv {
    /// Render the `subnet.env` file body.
    ///
    /// `FLANNEL_SUBNET` is the subnet's first usable IP (`.1`) at the subnet
    /// prefix, matching `WriteSubnetFile`'s `sn.IncrementIP()`.
    #[must_use]
    pub fn render(&self) -> String {
        let gw = self
            .subnet
            .nth_address(1)
            .unwrap_or_else(|_| self.subnet.network());
        let mut b = String::new();
        let _ = writeln!(b, "FLANNEL_NETWORK={}", self.network);
        let _ = writeln!(b, "FLANNEL_SUBNET={}/{}", gw, self.subnet.prefix_len());
        let _ = writeln!(b, "FLANNEL_MTU={}", self.mtu);
        let _ = writeln!(b, "FLANNEL_IPMASQ={}", self.ipmasq);
        b
    }

    /// Parse a `subnet.env` body back into a [`SubnetEnv`].
    ///
    /// The stored `FLANNEL_SUBNET` is the `.1/len` gateway form; we canonicalise
    /// it back to the `.0` network prefix so the round-trip yields the same
    /// [`Cidr`] the daemon leased.
    ///
    /// # Errors
    /// Returns an [`EnvParseError`] if a required key is missing or malformed.
    pub fn parse(body: &str) -> Result<Self, EnvParseError> {
        let mut network = None;
        let mut subnet = None;
        let mut mtu = None;
        let mut ipmasq = None;
        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (key, val) = line.split_once('=').ok_or(EnvParseError::Malformed)?;
            match key {
                "FLANNEL_NETWORK" => {
                    network = Some(val.parse::<Cidr>().map_err(|_| EnvParseError::Malformed)?);
                }
                "FLANNEL_SUBNET" => {
                    // ".1/len" → canonical ".0/len".
                    subnet = Some(val.parse::<Cidr>().map_err(|_| EnvParseError::Malformed)?);
                }
                "FLANNEL_MTU" => {
                    mtu = Some(val.parse::<u32>().map_err(|_| EnvParseError::Malformed)?);
                }
                "FLANNEL_IPMASQ" => {
                    ipmasq = Some(val.parse::<bool>().map_err(|_| EnvParseError::Malformed)?);
                }
                // Ignore unknown / dual-stack keys we do not model here.
                _ => {}
            }
        }
        Ok(Self {
            network: network.ok_or(EnvParseError::MissingKey("FLANNEL_NETWORK"))?,
            subnet: subnet.ok_or(EnvParseError::MissingKey("FLANNEL_SUBNET"))?,
            mtu: mtu.ok_or(EnvParseError::MissingKey("FLANNEL_MTU"))?,
            ipmasq: ipmasq.ok_or(EnvParseError::MissingKey("FLANNEL_IPMASQ"))?,
        })
    }
}

/// An error parsing a `subnet.env` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvParseError {
    /// A required `FLANNEL_*` key was absent.
    MissingKey(&'static str),
    /// A line or value was malformed.
    Malformed,
}

impl std::fmt::Display for EnvParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingKey(k) => write!(f, "subnet.env missing required key {k}"),
            Self::Malformed => write!(f, "subnet.env line is malformed"),
        }
    }
}

impl std::error::Error for EnvParseError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn cidr(s: &str) -> Cidr {
        Cidr::from_str(s).expect("cidr")
    }

    #[test]
    fn renders_flannel_defaults_with_gateway_subnet() {
        let env = SubnetEnv {
            network: cidr("10.42.0.0/16"),
            subnet: cidr("10.42.1.0/24"),
            mtu: 1450,
            ipmasq: false,
        };
        let body = env.render();
        assert_eq!(
            body,
            "FLANNEL_NETWORK=10.42.0.0/16\n\
             FLANNEL_SUBNET=10.42.1.1/24\n\
             FLANNEL_MTU=1450\n\
             FLANNEL_IPMASQ=false\n"
        );
    }

    #[test]
    fn parse_round_trips_through_canonical_subnet() {
        let env = SubnetEnv {
            network: cidr("10.42.0.0/16"),
            subnet: cidr("10.42.1.0/24"),
            mtu: 1450,
            ipmasq: true,
        };
        let parsed = SubnetEnv::parse(&env.render()).expect("parse");
        // FLANNEL_SUBNET=10.42.1.1/24 canonicalises back to 10.42.1.0/24.
        assert_eq!(parsed, env);
    }

    #[test]
    fn parse_tolerates_unknown_keys_and_blank_lines() {
        let body = "FLANNEL_NETWORK=10.42.0.0/16\n\
                    \n\
                    FLANNEL_SUBNET=10.42.2.1/24\n\
                    FLANNEL_IPV6_NETWORK=fd00::/48\n\
                    FLANNEL_MTU=1450\n\
                    FLANNEL_IPMASQ=true\n";
        let env = SubnetEnv::parse(body).expect("parse");
        assert_eq!(env.subnet, cidr("10.42.2.0/24"));
        assert!(env.ipmasq);
    }

    #[test]
    fn parse_rejects_missing_key() {
        let body = "FLANNEL_NETWORK=10.42.0.0/16\nFLANNEL_MTU=1450\nFLANNEL_IPMASQ=false\n";
        assert!(matches!(
            SubnetEnv::parse(body),
            Err(EnvParseError::MissingKey("FLANNEL_SUBNET"))
        ));
    }

    #[test]
    fn parse_rejects_malformed_line() {
        assert_eq!(SubnetEnv::parse("not-a-kv-line"), Err(EnvParseError::Malformed));
    }
}
