// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl camera — View cameras and look at recordings.
//!
//! Phase 1 stub. Backed by cave-home-camera crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("camera")
        .about("View cameras and look at recordings")
        .subcommand(
            Command::new("view")
                .about("Open the live stream")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("snapshot")
                .about("Save a still image now")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("clips")
                .about("List recent motion clips")
                .arg(Arg::new("name").long("name").required(true))
        )
        .subcommand(
            Command::new("list")
                .about("List every camera by location")
        )
}

pub fn run() -> i32 {
    println!("camera: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["view", "snapshot", "clips", "list"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `camera`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
