// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl notify — Send a notification.
//!
//! Phase 1 stub. Backed by cave-home-notify crate; wires the
//! clap surface and grandma-friendly verb names. Real implementation
//! lands once the backend exposes its public RPC surface.

use clap::{Arg, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("notify")
        .about("Send a notification")
        .subcommand(
            Command::new("send")
                .about("Send a notification to a person / channel")
                .arg(Arg::new("to").long("to").required(true))
                .arg(Arg::new("message").long("message").required(true))
        )
        .subcommand(
            Command::new("channels")
                .about("List notification channels")
        )
}

pub fn run() -> i32 {
    println!("notify: backend not yet attached — Phase 2.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_lists_every_subcommand() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for sub in ["send", "channels"] {
            assert!(
                names.iter().any(|n| n == sub),
                "subcommand {sub} missing from `notify`"
            );
        }
    }

    #[test]
    fn run_returns_zero() {
        assert_eq!(run(), 0);
    }
}
