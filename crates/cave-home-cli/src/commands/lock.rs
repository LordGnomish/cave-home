// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl lock — Control smart locks.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-lock crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("lock")
        .about("Control smart locks")
        .subcommand(
            Command::new("lock")
                .about("Lock a door")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("unlock")
                .about("Unlock a door")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("status")
                .about("Show lock status")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("list")
                .about("List every lock grouped by door")
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("lock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["lock", "unlock", "status", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `lock`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
