// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl display — TV and dashboard displays.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-display crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("display")
        .about("TV and dashboard displays")
        .subcommand(
            Command::new("on")
                .about("Turn a display on")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("off")
                .about("Turn a display off")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("cast")
                .about("Cast a URL or dashboard")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("url").long("url").required(true))
        )
        .subcommand(
            Command::new("list")
                .about("List every display")
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("display")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["on", "off", "cast", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `display`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
