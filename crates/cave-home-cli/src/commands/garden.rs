// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl garden — Irrigation and soil sensors.
//!
//! Phase 1 stub. Backed by cave-home-garden crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("garden")
        .about("Irrigation and soil sensors")
        .subcommand(
            Command::new("water")
                .about("Start watering a zone")
                .arg(Arg::new("zone").long("zone").required(true))
                .arg(Arg::new("minutes").long("minutes").required(true)),
        )
        .subcommand(
            Command::new("stop")
                .about("Stop watering")
                .arg(Arg::new("zone").long("zone").required(true)),
        )
        .subcommand(Command::new("schedule").about("Show irrigation schedule"))
        .subcommand(Command::new("list").about("List zones with moisture readings"))
}

pub fn run() -> i32 {
    println!("garden: backend not yet attached — Phase 2.");
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
        for sub in ["water", "stop", "schedule", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `garden`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
