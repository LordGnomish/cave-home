// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl matter — Matter Fabric — commission and pair devices.
//!
//! Phase 1 stub. Backed by cave-home-matter crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("matter")
        .about("Matter Fabric — commission and pair devices")
        .subcommand(
            Command::new("commission")
                .about("Commission a Matter device")
                .arg(Arg::new("code").long("code").required(true)),
        )
        .subcommand(Command::new("fabric").about("Show the local Matter Fabric"))
        .subcommand(Command::new("list").about("List paired Matter devices"))
}

pub fn run() -> i32 {
    println!("matter: backend not yet attached — Phase 2.");
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
        for sub in ["commission", "fabric", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `matter`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
