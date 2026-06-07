// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl cover — Control blinds, curtains, and garage doors.
//!
//! Phase 1 stub. Backed by cave-home-cover crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("cover")
        .about("Control blinds, curtains, and garage doors")
        .subcommand(
            Command::new("open")
                .about("Open a cover")
                .arg(Arg::new("name").long("name").required(true)),
        )
        .subcommand(
            Command::new("close")
                .about("Close a cover")
                .arg(Arg::new("name").long("name").required(true)),
        )
        .subcommand(
            Command::new("stop")
                .about("Stop a moving cover")
                .arg(Arg::new("name").long("name").required(true)),
        )
        .subcommand(
            Command::new("position")
                .about("Set position 0-100 (0=closed, 100=open)")
                .arg(Arg::new("name").long("name").required(true))
                .arg(Arg::new("value").long("value").required(true)),
        )
        .subcommand(Command::new("list").about("List every cover grouped by room"))
}

pub fn run() -> i32 {
    println!("cover: backend not yet attached — Phase 2.");
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
        for sub in ["open", "close", "stop", "position", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `cover`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
