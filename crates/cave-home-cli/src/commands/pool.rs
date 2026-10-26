// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl pool — Pool and spa controls.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-pool crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("pool")
        .about("Pool and spa controls")
        .subcommand(
            Command::new("status")
                .about("Show pool temperature + chemistry")
        )
        .subcommand(
            Command::new("pump")
                .about("Pump on / off")
                .arg(Arg::new("value").long("value").required(true))
        )
        .subcommand(
            Command::new("heater")
                .about("Heater on / off")
                .arg(Arg::new("value").long("value").required(true))
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("pool")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["status", "pump", "heater"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `pool`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
