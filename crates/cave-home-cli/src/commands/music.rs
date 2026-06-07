// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl music — Music playback (Music Assistant).
//!
//! Phase 1 stub. Backed by cave-home-music crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("music")
        .about("Music playback (Music Assistant)")
        .subcommand(
            Command::new("play")
                .about("Play a queue / artist / playlist")
                .arg(Arg::new("what").long("what").required(true))
        )
        .subcommand(
            Command::new("pause")
                .about("Pause playback")
        )
        .subcommand(
            Command::new("skip")
                .about("Skip to next track")
        )
        .subcommand(
            Command::new("volume")
                .about("Set volume 0-100")
                .arg(Arg::new("level").long("level").required(true))
        )
        .subcommand(
            Command::new("rooms")
                .about("List rooms that are playing")
        )
}

pub fn run() -> i32 {
    println!("music: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["play", "pause", "skip", "volume", "rooms"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `music`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
