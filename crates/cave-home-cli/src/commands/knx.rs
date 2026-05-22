// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// F4 KNX command surface — backed by cave-home-knx crate.

use clap::{Arg, Command};

/// Build the `cavehomectl knx <verb>` subcommand tree.
#[must_use]
pub fn cmd() -> Command {
    Command::new("knx")
        .about("KNX/IP bus control (routing + tunnelling)")
        .subcommand(Command::new("bus").about("Show KNX bus status (routing + tunnel)"))
        .subcommand(
            Command::new("group")
                .about("Read / write a KNX group address")
                .arg(Arg::new("ga").long("ga").required(true))
                .arg(Arg::new("value").long("value").required(false))
                .arg(Arg::new("dpt").long("dpt").required(false))
        )
        .subcommand(
            Command::new("monitor")
                .about("Tail live KNX telegrams (routing multicast + tunnel ind)")
                .arg(Arg::new("filter").long("filter").required(false))
        )
        .subcommand(
            Command::new("connect")
                .about("Open a tunnel to a KNX/IP server")
                .arg(Arg::new("host").long("host").required(true))
                .arg(Arg::new("port").long("port").default_value("3671"))
        )
        .subcommand(Command::new("disconnect").about("Close the active KNX/IP tunnel"))
        .subcommand(Command::new("scan").about("Discover KNX/IP devices on the LAN"))
}

/// Default no-op handler — Phase 2 wires this to cave-home-knx::gateway.
pub fn run() -> i32 {
    println!("knx: gateway not yet attached — Phase 2b.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_has_all_phase1_subcommands() {
        let c = cmd();
        for sub in ["bus", "group", "monitor", "connect", "disconnect", "scan"] {
            assert!(
                c.get_subcommands().any(|s| s.get_name() == sub),
                "subcommand {sub} missing"
            );
        }
    }

    #[test]
    fn group_takes_ga_value_dpt() {
        let c = cmd();
        let group = c
            .get_subcommands()
            .find(|s| s.get_name() == "group")
            .unwrap();
        let args: Vec<_> = group
            .get_arguments()
            .map(|a| a.get_id().as_str().to_string())
            .collect();
        for name in ["ga", "value", "dpt"] {
            assert!(args.contains(&name.to_string()), "arg {name} missing on `group`");
        }
    }

    #[test]
    fn connect_defaults_to_knx_port_3671() {
        let c = cmd();
        let conn = c
            .get_subcommands()
            .find(|s| s.get_name() == "connect")
            .unwrap();
        let port = conn
            .get_arguments()
            .find(|a| a.get_id().as_str() == "port")
            .unwrap();
        assert_eq!(
            port.get_default_values()
                .first()
                .map(|s| s.to_string_lossy().into_owned()),
            Some("3671".to_string())
        );
    }
}
