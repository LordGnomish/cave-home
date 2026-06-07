// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl wellness — Sleep, activity, and mood (today).
//!
//! Phase 1 stub. Backed by cave-home-wellness crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("wellness")
        .about("Sleep, activity, and mood (today)")
        .subcommand(Command::new("today").about("Show today's wellness summary"))
        .subcommand(
            Command::new("trend")
                .about("Show 7-day trend")
                .arg(Arg::new("metric").long("metric").required(false)),
        )
}

pub fn run() -> i32 {
    println!("wellness: backend not yet attached — Phase 2.");
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
        for sub in ["today", "trend"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `wellness`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
