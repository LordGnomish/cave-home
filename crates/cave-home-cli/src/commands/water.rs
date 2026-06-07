// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl water — Smart-water meters and leak sensors.
//!
//! Phase 1 stub. Backed by cave-home-water crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::Command;

#[must_use]
pub fn cmd() -> Command {
    Command::new("water")
        .about("Smart-water meters and leak sensors")
        .subcommand(Command::new("status").about("Show today's water usage + meter health"))
        .subcommand(Command::new("leaks").about("List active leak alerts"))
}

pub fn run() -> i32 {
    println!("water: backend not yet attached — Phase 2.");
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
        for sub in ["status", "leaks"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `water`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
