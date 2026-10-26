// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl voice — Talk to your home.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-voice crate; wires the
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
                .arg(Arg::new("text").long("text").required(true))
        )
        .subcommand(
            Command::new("listen")
                .about("Listen and transcribe one utterance")
        )
        .subcommand(
            Command::new("wake")
                .about("Show wake-word state")
        )
}

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("voice")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["say", "listen", "wake"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `voice`"
            );
        }
    }

    #[test]
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
