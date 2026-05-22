// SPDX-License-Identifier: Apache-2.0
//! CNI ADD/DEL/CHECK/VERSION dispatcher.
//!
//! Upstream parity: `cni-plugin/main.go::main` + `flannel.go::cmdAdd /
//! cmdDel / cmdCheck`. The dispatcher is pure (no I/O — the binary in
//! `src/bin/` does stdin/stdout).

use crate::cni::subnet_env::{SubnetEnv, parse_subnet_env};
use crate::cni::types::{CniResult, IpConfig, NetConf, Route, SUPPORTED_VERSIONS};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use thiserror::Error;

/// Wraps the subset of CNI env vars we care about.
///
/// Upstream parity: CNI SPEC §3.1 (the `CNI_*` variables).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CniInvocation {
    pub command: CniCommand,
    pub container_id: String,
    pub netns: String,
    pub ifname: String,
    pub args: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CniCommand {
    Add,
    Del,
    Check,
    Version,
}

impl CniCommand {
    pub fn parse(s: &str) -> Result<Self, CniError> {
        match s {
            "ADD" => Ok(Self::Add),
            "DEL" => Ok(Self::Del),
            "CHECK" => Ok(Self::Check),
            "VERSION" => Ok(Self::Version),
            other => Err(CniError::UnsupportedCommand(other.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CniRequest {
    pub invocation: CniInvocation,
    pub conf: NetConf,
    /// Pre-loaded subnet.env contents (the binary does the file read; tests
    /// inject directly).
    pub subnet_env: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CniResponse {
    Result(CniResult),
    Version(VersionInfo),
    Empty {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    #[serde(rename = "cniVersion")]
    pub cni_version: String,
    #[serde(rename = "supportedVersions")]
    pub supported_versions: Vec<String>,
}

/// Errors emitted by the dispatcher (also used by `subnet_env::parse_subnet_env`).
#[derive(Debug, Error)]
pub enum CniError {
    #[error("CNI parse error: {0}")]
    Parse(String),
    #[error("missing subnet.env (configured at {0})")]
    MissingSubnetEnv(String),
    #[error("unsupported CNI_COMMAND: {0}")]
    UnsupportedCommand(String),
    #[error("delegate plugin chaining not implemented in Phase 1 (HANDOFF: phase-1b)")]
    DelegateNotImplemented,
    #[error("io: {0}")]
    Io(String),
}

pub fn dispatch(req: &CniRequest) -> Result<CniResponse, CniError> {
    match req.invocation.command {
        CniCommand::Version => Ok(CniResponse::Version(VersionInfo {
            cni_version: req.conf.cni_version.clone(),
            supported_versions: SUPPORTED_VERSIONS.iter().map(|s| (*s).to_string()).collect(),
        })),
        CniCommand::Add => Ok(CniResponse::Result(build_add_result(req)?)),
        CniCommand::Check => Ok(CniResponse::Result(build_add_result(req)?)),
        CniCommand::Del => Ok(CniResponse::Empty {}),
    }
}

fn build_add_result(req: &CniRequest) -> Result<CniResult, CniError> {
    let env_str = req.subnet_env.as_deref().ok_or_else(|| {
        CniError::MissingSubnetEnv(req.conf.subnet_file.clone())
    })?;
    let env = parse_subnet_env(env_str)?;
    Ok(synthesize_result(&req.conf, &env))
}

/// Build the CNI result that would be returned to the runtime.
///
/// Phase 1: we surface the subnet/MTU/network values from `subnet.env` so
/// the kubelet can see what flannel would have programmed. Real delegate
/// chaining (invoking `bridge` as a child process) is Phase 1b — see
/// `parity.manifest.toml` [[unmapped]] `delegate-chain`.
pub fn synthesize_result(conf: &NetConf, env: &SubnetEnv) -> CniResult {
    let gateway = first_usable(env.subnet);
    CniResult {
        cni_version: conf.cni_version.clone(),
        interfaces: Vec::new(),
        ips: vec![IpConfig {
            address: IpNet::V4(env.subnet),
            gateway: gateway.map(IpAddr::V4),
            interface: None,
        }],
        routes: vec![Route {
            dst: IpNet::V4(env.network),
            gw: None,
        }],
        dns: None,
    }
}

/// First usable IPv4 host within `net` (i.e. `net.network() + 1`).
fn first_usable(net: ipnet::Ipv4Net) -> Option<std::net::Ipv4Addr> {
    let base = u32::from(net.network());
    base.checked_add(1).map(std::net::Ipv4Addr::from)
}
