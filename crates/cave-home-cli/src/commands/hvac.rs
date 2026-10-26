// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl hvac — Control thermostats and heat pumps.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-hvac crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("hvac")
        .about("Control thermostats and heat pumps")
        .subcommand(
            Command::new("set")
                .about("Set target temperature (°C)")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("temp").long("temp").required(true))
        )
        .subcommand(
            Command::new("mode")
                .about("Set mode (heat/cool/auto/off)")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("value").long("value").required(true))
        )
        .subcommand(
            Command::new("status")
                .about("Show current + target temp")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("list")
                .about("List every thermostat by room")
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("hvac")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["set", "mode", "status", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `hvac`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
