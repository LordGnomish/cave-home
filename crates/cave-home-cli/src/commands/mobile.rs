// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl mobile — Companion-app devices.
//!
//! Phase 1 stub. Backed by cave-home-mobile crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::Command;

#[must_use]
pub fn cmd() -> Command {
    Command::new("mobile")
        .about("Companion-app devices")
        .subcommand(
            Command::new("list")
                .about("List paired phones / tablets")
        )
        .subcommand(
            Command::new("pair")
                .about("Show pairing code + QR")
        )
}

pub fn run() -> i32 {
    println!("mobile: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["list", "pair"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `mobile`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
