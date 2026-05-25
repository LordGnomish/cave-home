// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl zigbee — Zigbee Coordinator controls.
//!
//! Phase 1 stub. Backed by cave-home-zigbee crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("zigbee")
        .about("Zigbee Coordinator controls")
        .subcommand(
            Command::new("pair")
                .about("Open pairing for N seconds")
                .arg(Arg::new("seconds").long("seconds").required(true))
        )
        .subcommand(
            Command::new("list")
                .about("List Zigbee devices")
        )
        .subcommand(
            Command::new("network")
                .about("Show coordinator + network info")
        )
}

pub fn run() -> i32 {
    println!("zigbee: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["pair", "list", "network"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `zigbee`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
