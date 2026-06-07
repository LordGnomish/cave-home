// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl doorbell — Doorbell controls and history.
//!
//! Phase 1 stub. Backed by cave-home-doorbell crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("doorbell")
        .about("Doorbell controls and history")
        .subcommand(
            Command::new("answer")
                .about("Open intercom to the door")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("history")
                .about("Show recent button presses")
        )
        .subcommand(
            Command::new("list")
                .about("List every doorbell")
        )
}

pub fn run() -> i32 {
    println!("doorbell: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["answer", "history", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `doorbell`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
