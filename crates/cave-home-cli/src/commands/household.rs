// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl household — Chores, shopping list, batteries.
//!
//! Phase 1 stub. Backed by cave-home-household crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("household")
        .about("Chores, shopping list, batteries")
        .subcommand(Command::new("chores").about("Show today's chores"))
        .subcommand(
            Command::new("shop")
                .about("Add to shopping list")
                .arg(Arg::new("item").long("item").required(true)),
        )
        .subcommand(
            Command::new("inventory")
                .about("Show food / battery inventory")
                .arg(Arg::new("kind").long("kind").required(false)),
        )
}

pub fn run() -> i32 {
    println!("household: backend not yet attached — Phase 2.");
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
        for sub in ["chores", "shop", "inventory"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `household`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
