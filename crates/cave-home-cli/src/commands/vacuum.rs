// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl vacuum — Control robot vacuums.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-vacuum crate; wires the
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

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("vacuum")
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
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
