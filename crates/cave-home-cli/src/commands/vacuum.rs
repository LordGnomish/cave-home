// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl vacuum — Control robot vacuums.
//!
//! Phase 1 stub. Backed by cave-home-vacuum crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("vacuum")
        .about("Control robot vacuums")
        .subcommand(
            Command::new("start")
                .about("Start a cleaning cycle")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("stop")
                .about("Stop the current cycle")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("dock")
                .about("Send back to dock")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("status")
                .about("Show battery + state")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("list")
                .about("List every vacuum")
        )
}

pub fn run() -> i32 {
    println!("vacuum: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["start", "stop", "dock", "status", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `vacuum`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
