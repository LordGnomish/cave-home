// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl hvac — Control thermostats and heat pumps.
//!
//! Phase 1 stub. Backed by cave-home-hvac crate; wires the
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
                .arg(Arg::new("temp").long("temp").required(true)),
        )
        .subcommand(
            Command::new("mode")
                .about("Set mode (heat/cool/auto/off)")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("value").long("value").required(true)),
        )
        .subcommand(
            Command::new("status")
                .about("Show current + target temp")
                .arg(Arg::new("name").long("name").required(true)),
        )
        .subcommand(Command::new("list").about("List every thermostat by room"))
}

pub fn run() -> i32 {
    println!("hvac: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();
        for sub in ["set", "mode", "status", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `hvac`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
