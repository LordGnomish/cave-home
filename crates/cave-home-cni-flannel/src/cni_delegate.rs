// SPDX-License-Identifier: Apache-2.0
//! CNI delegate-config generation — the flannel `/opt/cni/bin/flannel` plugin's
//! core behaviour (flannel-io/cni-plugin `flannel.go`).
//!
//! The flannel CNI plugin is itself a *meta* plugin: it does not plumb the veth
//! itself. On ADD it reads `/run/flannel/subnet.env` (see [`crate::subnet_env`])
//! and the incoming network config, then synthesises a *delegate* netconf for
//! the `bridge` plugin with `host-local` IPAM scoped to this node's subnet, and
//! execs it. This module ports the part that is pure computation: turning a
//! [`SubnetEnv`] (+ the cluster network name / CNI version) into that delegate
//! JSON. Exec-ing the delegate and the stdin/stdout CNI wire protocol are the
//! binary's edge (see `bin/flannel.rs`).
//!
//! The delegate it builds (flannel defaults — bridge `cni0`, `isGateway`,
//! host-local IPAM over the node subnet with a route to the whole pod network):
//!
//! ```json
//! {"name":"cbr0","cniVersion":"1.0.0","type":"bridge","mtu":1450,
//!  "ipMasq":false,"isGateway":true,
//!  "ipam":{"type":"host-local",
//!          "ranges":[[{"subnet":"10.42.1.0/24"}]],
//!          "routes":[{"dst":"10.42.0.0/16"}]}}
//! ```
//!
//! flannel deliberately sets the delegate's `ipMasq` to `false` — masquerade is
//! the daemon's job (iptables), not the bridge's.

use std::fmt::Write as _;

use crate::subnet_env::SubnetEnv;

/// The default bridge name flannel's delegate uses.
pub const DEFAULT_BRIDGE: &str = "cni0";
/// The default delegate plugin type.
pub const DELEGATE_TYPE: &str = "bridge";

/// Inputs to the delegate config beyond what `subnet.env` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegateConfig {
    /// The CNI network name (from the incoming netconf `name`).
    pub name: String,
    /// The CNI spec version to echo (`cniVersion`).
    pub cni_version: String,
    /// The bridge device name (`bridge`).
    pub bridge: String,
}

impl DelegateConfig {
    /// flannel defaults: network name `cbr0`, CNI v1.0.0, bridge `cni0`.
    #[must_use]
    pub fn defaults(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            cni_version: "1.0.0".to_owned(),
            bridge: DEFAULT_BRIDGE.to_owned(),
        }
    }

    /// Derive the delegate config from the incoming CNI netconf JSON (read on
    /// the plugin's stdin), extracting `name` and `cniVersion` and falling back
    /// to the flannel defaults for anything absent.
    #[must_use]
    pub fn from_netconf(netconf: &str) -> Self {
        let name = json_string_field(netconf, "name").unwrap_or_else(|| "cbr0".to_owned());
        let cni_version =
            json_string_field(netconf, "cniVersion").unwrap_or_else(|| "1.0.0".to_owned());
        Self {
            name,
            cni_version,
            bridge: DEFAULT_BRIDGE.to_owned(),
        }
    }

    /// Build the delegate netconf JSON for the `bridge` plugin from this node's
    /// `subnet.env`.
    ///
    /// `ipam.ranges` is scoped to the node subnet (canonical network/prefix);
    /// `ipam.routes` routes the whole pod network through the bridge; `mtu`
    /// comes from `FLANNEL_MTU`. The delegate's own `ipMasq` is always `false`
    /// (the daemon masquerades).
    #[must_use]
    pub fn build(&self, env: &SubnetEnv) -> String {
        let node_subnet = format!("{}/{}", env.subnet.network(), env.subnet.prefix_len());
        let pod_network = format!("{}/{}", env.network.network(), env.network.prefix_len());
        let mut j = String::new();
        let _ = write!(
            j,
            "{{\"name\":\"{}\",\"cniVersion\":\"{}\",\"type\":\"{}\",\
             \"mtu\":{},\"ipMasq\":false,\"isGateway\":true,\
             \"bridge\":\"{}\",\
             \"ipam\":{{\"type\":\"host-local\",\
             \"ranges\":[[{{\"subnet\":\"{}\"}}]],\
             \"routes\":[{{\"dst\":\"{}\"}}]}}}}",
            self.name, self.cni_version, DELEGATE_TYPE, env.mtu, self.bridge, node_subnet,
            pod_network
        );
        j
    }
}

/// Extract a quoted-string top-level JSON field (`"key":"value"`), tolerant of
/// surrounding whitespace. Sufficient for the small CNI netconf fields we read.
fn json_string_field(s: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let after = &s[s.find(&needle)? + needle.len()..];
    let after = after.trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    let rest = after.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cidr::Cidr;
    use std::str::FromStr;

    fn env() -> SubnetEnv {
        SubnetEnv {
            network: Cidr::from_str("10.42.0.0/16").expect("net"),
            subnet: Cidr::from_str("10.42.1.0/24").expect("sn"),
            mtu: 1450,
            ipmasq: false,
        }
    }

    #[test]
    fn builds_bridge_delegate_scoped_to_node_subnet() {
        let cfg = DelegateConfig::defaults("cbr0");
        let j = cfg.build(&env());
        assert!(j.contains("\"type\":\"bridge\""));
        assert!(j.contains("\"name\":\"cbr0\""));
        assert!(j.contains("\"cniVersion\":\"1.0.0\""));
        assert!(j.contains("\"mtu\":1450"));
        assert!(j.contains("\"bridge\":\"cni0\""));
        // host-local IPAM over the node /24, route to the whole /16.
        assert!(j.contains("\"type\":\"host-local\""));
        assert!(j.contains("\"subnet\":\"10.42.1.0/24\""));
        assert!(j.contains("\"dst\":\"10.42.0.0/16\""));
    }

    #[test]
    fn delegate_ipmasq_is_always_false() {
        // Even when the daemon enables masquerade, the bridge delegate must not.
        let mut e = env();
        e.ipmasq = true;
        let j = DelegateConfig::defaults("cbr0").build(&e);
        assert!(j.contains("\"ipMasq\":false"));
    }

    #[test]
    fn from_netconf_extracts_name_and_version() {
        let nc = r#"{ "cniVersion": "1.0.0", "name": "mynet", "type": "flannel" }"#;
        let cfg = DelegateConfig::from_netconf(nc);
        assert_eq!(cfg.name, "mynet");
        assert_eq!(cfg.cni_version, "1.0.0");
        assert_eq!(cfg.bridge, "cni0");
    }

    #[test]
    fn from_netconf_falls_back_to_defaults() {
        let cfg = DelegateConfig::from_netconf("{}");
        assert_eq!(cfg.name, "cbr0");
        assert_eq!(cfg.cni_version, "1.0.0");
    }

    #[test]
    fn node_subnet_is_canonical_network_not_gateway() {
        // env.subnet is canonical (.0); the delegate must scope IPAM to the
        // network, not a host address.
        let j = DelegateConfig::defaults("cbr0").build(&env());
        assert!(j.contains("\"subnet\":\"10.42.1.0/24\""));
        assert!(!j.contains("10.42.1.1/24"));
    }
}
