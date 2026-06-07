// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl zwave — Z-Wave controller.
//!
//! Phase 1 stub. Backed by cave-home-zwave crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::Command;

#[must_use]
pub fn cmd() -> Command {
    Command::new("zwave")
        .about("Z-Wave controller")
        .subcommand(
            Command::new("pair")
                .about("Add a Z-Wave node")
        )
        .subcommand(
            Command::new("list")
                .about("List Z-Wave nodes")
        )
        .subcommand(
            Command::new("heal")
                .about("Heal the Z-Wave mesh")
        )
}

pub fn run() -> i32 {
    println!("zwave: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["pair", "list", "heal"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `zwave`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
