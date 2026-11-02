// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl history — Look at sensor history.
//!
//! Phase 1 stub. Backed by cave-home-history crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("history")
        .about("Look at sensor history")
        .subcommand(
            Command::new("show")
                .about("Show recent values for a sensor")
                .arg(Arg::new("name").long("name").required(true)),
        )
        .subcommand(
            Command::new("export")
                .about("Export a range to CSV")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("from").long("from").required(true))
                .arg(Arg::new("to").long("to").required(true)),
        )
}

pub fn run() -> i32 {
    println!("history: backend not yet attached — Phase 2.");
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
        for sub in ["show", "export"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `history`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
