// SPDX-License-Identifier: Apache-2.0
//! `/opt/cni/bin/flannel` — the flannel CNI meta-plugin entry point.
//!
//! The container runtime invokes this binary per pod with `CNI_COMMAND` in the
//! environment and the network config JSON on stdin. flannel's plugin reads
//! `/run/flannel/subnet.env` (written by the daemon after it leased this node's
//! pod subnet), synthesises a `bridge` + `host-local` delegate netconf scoped to
//! that subnet, and (in the real plugin) execs the delegate. Here we do the
//! pure part faithfully — parse the env + netconf and emit the delegate JSON on
//! stdout — which is what the runtime's CNI library would then run.
//!
//! All the logic lives in the library ([`cave_home_cni_flannel::subnet_env`] +
//! [`cave_home_cni_flannel::cni_delegate`]) and is unit-tested there; this file
//! is only the I/O edge.

use std::io::Read as _;
use std::process::ExitCode;

use cave_home_cni_flannel::cni_delegate::DelegateConfig;
use cave_home_cni_flannel::subnet_env::SubnetEnv;

/// Where the daemon writes this node's lease contract.
const DEFAULT_SUBNET_ENV: &str = "/run/flannel/subnet.env";

fn main() -> ExitCode {
    let command = std::env::var("CNI_COMMAND").unwrap_or_default();

    // The daemon's subnet.env path is overridable for testing / non-default
    // installs via FLANNEL_SUBNET_FILE.
    let env_path =
        std::env::var("FLANNEL_SUBNET_FILE").unwrap_or_else(|_| DEFAULT_SUBNET_ENV.to_owned());

    let mut netconf = String::new();
    if std::io::stdin().read_to_string(&mut netconf).is_err() {
        eprintln!("flannel cni: failed to read netconf from stdin");
        return ExitCode::FAILURE;
    }

    match command.as_str() {
        // ADD / DEL / CHECK all build the same delegate; the runtime execs it
        // with the corresponding command. VERSION is handled by the framework.
        "ADD" | "DEL" | "CHECK" => {
            let body = match std::fs::read_to_string(&env_path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("flannel cni: cannot read {env_path}: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let env = match SubnetEnv::parse(&body) {
                Ok(env) => env,
                Err(e) => {
                    eprintln!("flannel cni: invalid {env_path}: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let delegate = DelegateConfig::from_netconf(&netconf).build(&env);
            println!("{delegate}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("flannel cni: unsupported CNI_COMMAND '{other}'");
            ExitCode::FAILURE
        }
    }
}
