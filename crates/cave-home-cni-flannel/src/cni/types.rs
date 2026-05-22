// SPDX-License-Identifier: Apache-2.0
//! CNI spec types — the JSON wire format.
//!
//! Upstream parity: <https://github.com/containernetworking/cni/blob/main/SPEC.md>
//! plus the flannel-specific `NetConf` from
//! <https://github.com/flannel-io/cni-plugin/blob/main/flannel.go>.

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

/// Top-level CNI network config (read from stdin).
///
/// Upstream parity: `cni-plugin/flannel.go::NetConf`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetConf {
    #[serde(rename = "cniVersion")]
    pub cni_version: String,
    pub name: String,
    #[serde(rename = "type")]
    pub plugin_type: String,
    /// Path to the subnet.env file; defaults to `/run/flannel/subnet.env`.
    #[serde(rename = "subnetFile", default = "default_subnet_file")]
    pub subnet_file: String,
    /// Path to the data dir for ADD→DEL state; defaults to `/var/lib/cni/flannel`.
    #[serde(rename = "dataDir", default = "default_data_dir")]
    pub data_dir: String,
    /// IPAM block (overridden when delegating); flannel pulls IPAM from the
    /// host-local plugin via subnet.env so this is rarely set in practice.
    #[serde(default)]
    pub ipam: Option<serde_json::Value>,
    /// Delegate plugin config (typically `bridge`). Mandatory in real
    /// deployments; Phase 1 just echoes this back into the result so the
    /// caller (kubelet) can see what would be delegated.
    #[serde(default)]
    pub delegate: Option<serde_json::Value>,
    /// IPMASQ override (defaults to subnet.env value).
    #[serde(rename = "ipMasq", default)]
    pub ip_masq: Option<bool>,
    /// MTU override (defaults to subnet.env value).
    #[serde(default)]
    pub mtu: Option<u32>,
    #[serde(rename = "runtimeConfig", default)]
    pub runtime_config: Option<serde_json::Value>,
}

fn default_subnet_file() -> String {
    "/run/flannel/subnet.env".into()
}

fn default_data_dir() -> String {
    "/var/lib/cni/flannel".into()
}

/// CNI ADD/CHECK result.
///
/// Upstream parity: CNI SPEC §5 ("Result Type").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniResult {
    #[serde(rename = "cniVersion")]
    pub cni_version: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<Interface>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ips: Vec<IpConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns: Option<Dns>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interface {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpConfig {
    pub address: IpNet,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<std::net::IpAddr>,
    /// Index into `result.interfaces`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub dst: IpNet,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gw: Option<std::net::IpAddr>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dns {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nameservers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

/// Supported CNI spec versions for VERSION command.
///
/// Upstream parity: `cni-plugin` declares `0.1.0..1.0.0`. We mirror.
pub const SUPPORTED_VERSIONS: &[&str] = &["0.1.0", "0.2.0", "0.3.0", "0.3.1", "0.4.0", "1.0.0"];
