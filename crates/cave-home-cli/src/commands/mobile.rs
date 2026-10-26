// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl mobile — Companion-app devices.
//!
//! Not connected yet: every verb exits `EXIT_NOT_CONNECTED` (69) and changes
//! nothing. Will be backed by cave-home-mobile crate; wires the
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

/// No backend is wired into the CLI for this verb yet: say so, change nothing.
#[must_use]
pub fn run() -> i32 {
    crate::commands::not_connected::not_connected("mobile")
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
    fn run_reports_not_connected() {
        assert_eq!(run(), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
