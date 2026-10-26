// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl lock — Control smart locks.
//!
//! Phase 1 stub. Backed by cave-home-lock crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("lock")
        .about("Control smart locks")
        .subcommand(
            Command::new("lock")
                .about("Lock a door")
                .arg(Arg::new("name").long("name").required(true)),
        )
        .subcommand(
            Command::new("unlock")
                .about("Unlock a door")
                .arg(Arg::new("name").long("name").required(true)),
        )
        .subcommand(
            Command::new("status")
                .about("Show lock status")
                .arg(Arg::new("name").long("name").required(true)),
        )
        .subcommand(Command::new("list").about("List every lock grouped by door"))
}

pub fn run() -> i32 {
    println!("lock: backend not yet attached — Phase 2.");
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
        for sub in ["lock", "unlock", "status", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `lock`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
