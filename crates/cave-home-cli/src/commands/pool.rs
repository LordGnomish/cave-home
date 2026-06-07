// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl pool — Pool and spa controls.
//!
//! Phase 1 stub. Backed by cave-home-pool crate; wires the
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

pub fn run() -> i32 {
    println!("pool: backend not yet attached — Phase 2.");
    0
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
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
