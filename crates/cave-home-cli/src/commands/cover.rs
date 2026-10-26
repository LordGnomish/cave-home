// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl cover — Control blinds, curtains, and garage doors.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-cover crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("cover")
        .about("Control blinds, curtains, and garage doors")
        .subcommand(
            Command::new("open")
                .about("Open a cover")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("close")
                .about("Close a cover")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("stop")
                .about("Stop a moving cover")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("position")
                .about("Set position 0-100 (0=closed, 100=open)")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("value").long("value").required(true))
        )
        .subcommand(
            Command::new("list")
                .about("List every cover grouped by room")
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("cover")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["open", "close", "stop", "position", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `cover`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
