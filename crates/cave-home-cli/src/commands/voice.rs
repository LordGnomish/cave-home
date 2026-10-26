// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl voice — Talk to your home.
//!
//! Phase 1 stub. Backed by cave-home-voice crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("voice")
        .about("Talk to your home")
        .subcommand(
            Command::new("say")
                .about("Speak a phrase out loud")
                .arg(Arg::new("text").long("text").required(true)),
        )
        .subcommand(Command::new("listen").about("Listen and transcribe one utterance"))
        .subcommand(Command::new("wake").about("Show wake-word state"))
}

pub fn run() -> i32 {
    println!("voice: backend not yet attached — Phase 2.");
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
        for sub in ["say", "listen", "wake"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `voice`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
