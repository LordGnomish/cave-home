// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl household — Chores, shopping list, batteries.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-household crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("household")
        .about("Chores, shopping list, batteries")
        .subcommand(
            Command::new("chores")
                .about("Show today's chores")
        )
        .subcommand(
            Command::new("shop")
                .about("Add to shopping list")
                .arg(Arg::new("item").long("item").required(true))
        )
        .subcommand(
            Command::new("inventory")
                .about("Show food / battery inventory")
                .arg(Arg::new("kind").long("kind").required(false))
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("household")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["chores", "shop", "inventory"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `household`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
