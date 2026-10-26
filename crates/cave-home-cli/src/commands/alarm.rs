// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl alarm — Intruder-alarm panel controls.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-alarm crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("alarm")
        .about("Intruder-alarm panel controls")
        .subcommand(
            Command::new("arm")
                .about("Arm (home / away / night)")
                .arg(Arg::new("mode").long("mode").required(true))
        )
        .subcommand(
            Command::new("disarm")
                .about("Disarm the alarm")
        )
        .subcommand(
            Command::new("status")
                .about("Show armed state + history")
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("alarm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["arm", "disarm", "status"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `alarm`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
