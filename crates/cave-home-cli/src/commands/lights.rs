// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl lights — Control your lights (on/off/dim/color).
//!
//! Phase 1 stub. Backed by cave-home-lights crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("lights")
        .about("Control your lights (on/off/dim/color)")
        .subcommand(
            Command::new("on")
                .about("Turn a light on")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("off")
                .about("Turn a light off")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("dim")
                .about("Set brightness 0-100")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("level").long("level").required(true))
        )
        .subcommand(
            Command::new("color")
                .about("Set color (e.g. warm/cool/red/#RRGGBB)")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("value").long("value").required(true))
        )
        .subcommand(
            Command::new("list")
                .about("List every light grouped by room")
        )
}

pub fn run() -> i32 {
    println!("lights: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["on", "off", "dim", "color", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `lights`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
