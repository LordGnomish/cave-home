// SPDX-License-Identifier: Apache-2.0
//! CNI plugin entry point.
//!
//! Upstream parity: `cni-plugin/main.go::main`.
//!
//! Protocol (CNI SPEC §3):
//!   - `CNI_COMMAND` env var → ADD/DEL/CHECK/VERSION
//!   - `CNI_CONTAINERID`, `CNI_NETNS`, `CNI_IFNAME`, `CNI_ARGS`, `CNI_PATH` env
//!   - JSON config on stdin
//!   - JSON result on stdout (or non-zero exit + JSON error on stderr).

use cave_home_cni_flannel::cni::{
    CniError, CniInvocation, CniRequest, CniResponse, dispatch,
};
use cave_home_cni_flannel::cni::handler::CniCommand;
use cave_home_cni_flannel::cni::types::NetConf;
use serde::Serialize;
use std::env;
use std::io::Read;
use std::process::ExitCode;
use tokio::fs;

#[derive(Serialize)]
struct CniErrorReply<'a> {
    #[serde(rename = "cniVersion")]
    cni_version: &'a str,
    code: u32,
    msg: &'a str,
    details: String,
}

fn read_stdin_blocking() -> Result<String, CniError> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| CniError::Io(e.to_string()))?;
    Ok(buf)
}

fn parse_invocation() -> Result<CniInvocation, CniError> {
    let command = CniCommand::parse(
        &env::var("CNI_COMMAND").map_err(|_| CniError::Parse("CNI_COMMAND not set".into()))?,
    )?;
    Ok(CniInvocation {
        command,
        container_id: env::var("CNI_CONTAINERID").unwrap_or_default(),
        netns: env::var("CNI_NETNS").unwrap_or_default(),
        ifname: env::var("CNI_IFNAME").unwrap_or_default(),
        args: env::var("CNI_ARGS").unwrap_or_default(),
        path: env::var("CNI_PATH").unwrap_or_default(),
    })
}

async fn run() -> Result<CniResponse, CniError> {
    let invocation = parse_invocation()?;
    let stdin = read_stdin_blocking()?;
    let conf: NetConf = if invocation.command == CniCommand::Version && stdin.trim().is_empty() {
        NetConf {
            cni_version: "1.0.0".into(),
            name: "flannel".into(),
            plugin_type: "flannel".into(),
            subnet_file: "/run/flannel/subnet.env".into(),
            data_dir: "/var/lib/cni/flannel".into(),
            ipam: None,
            delegate: None,
            ip_masq: None,
            mtu: None,
            runtime_config: None,
        }
    } else {
        serde_json::from_str(&stdin).map_err(|e| CniError::Parse(e.to_string()))?
    };

    // Read subnet.env for ADD/CHECK; DEL+VERSION skip.
    let subnet_env = match invocation.command {
        CniCommand::Add | CniCommand::Check => {
            Some(fs::read_to_string(&conf.subnet_file).await.map_err(|_| {
                CniError::MissingSubnetEnv(conf.subnet_file.clone())
            })?)
        }
        _ => None,
    };

    dispatch(&CniRequest {
        invocation,
        conf,
        subnet_env,
    })
}

fn emit_response(resp: &CniResponse) -> Result<(), CniError> {
    let s = serde_json::to_string(resp).map_err(|e| CniError::Io(e.to_string()))?;
    println!("{s}");
    Ok(())
}

fn emit_error(err: &CniError) {
    let reply = CniErrorReply {
        cni_version: "1.0.0",
        code: match err {
            CniError::Parse(_) => 1,
            CniError::MissingSubnetEnv(_) => 7,
            CniError::UnsupportedCommand(_) => 4,
            CniError::DelegateNotImplemented => 6,
            CniError::Io(_) => 11,
        },
        msg: "flannel CNI failed",
        details: err.to_string(),
    };
    if let Ok(s) = serde_json::to_string(&reply) {
        eprintln!("{s}");
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(resp) => {
            if let Err(e) = emit_response(&resp) {
                emit_error(&e);
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            emit_error(&e);
            ExitCode::FAILURE
        }
    }
}
