// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl matter — Matter Fabric — commission and pair devices.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-matter crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("matter")
        .about("Matter Fabric — commission and pair devices")
        .subcommand(
            Command::new("commission")
                .about("Commission a Matter device")
                .arg(Arg::new("code").long("code").required(true))
        )
        .subcommand(
            Command::new("fabric")
                .about("Show the local Matter Fabric")
        )
        .subcommand(
            Command::new("list")
                .about("List paired Matter devices")
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("matter")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["commission", "fabric", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `matter`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
