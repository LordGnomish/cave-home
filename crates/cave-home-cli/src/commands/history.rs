// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl history — Look at sensor history.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-history crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("history")
        .about("Look at sensor history")
        .subcommand(
            Command::new("show")
                .about("Show recent values for a sensor")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("export")
                .about("Export a range to CSV")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("from").long("from").required(true))
                .arg(Arg::new("to").long("to").required(true))
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("history")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["show", "export"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `history`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
