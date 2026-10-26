// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl join <token>` — join a running cluster.
//!
//! ADR-005 §a: the CLI is the power-user join surface; the same
//! primitive sits behind the OS image's first-boot wizard and the
//! Portal "Add node" wizard. ADR-007: the raw token is technical;
//! the user-visible output stays grandma-friendly.

use cave_home_node_discovery::{JoinToken, TokenError};
use clap::{Arg, ArgMatches, Command};

/// Build the clap subtree for `join`.
#[must_use]
pub fn cmd() -> Command {
    Command::new("join")
        .about("Join this node to a running cave-home cluster")
        .arg(
            Arg::new("token")
                .help("Join token (from the primary hub's 'Add node' QR / wizard)")
                .required(true)
                .num_args(1),
        )
        .arg(
            Arg::new("server")
                .long("server")
                .help("Primary hub URL (default: auto-discover on LAN)")
                .num_args(1),
        )
}

/// Check the token's shape with the same parser the hub uses
/// (`cave-home-node-discovery`'s [`JoinToken`]: 12 characters of its
/// Crockford-style charset). Expiry and the real handshake are the hub's job.
///
/// # Errors
/// The parser's [`TokenError`].
pub fn check_token(token: &str) -> Result<(), TokenError> {
    // Lifetime is unknown client-side; any non-zero value checks shape only.
    JoinToken::parse(token.trim(), 0, 1).map(|_| ())
}

/// Dispatcher entry. Validates the token shape, then reports that the join
/// transport is not connected — there is no `cave-home-cluster` join call to
/// make yet, so the CLI never claims "Connected".
#[must_use]
pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    let token = matches
        .get_one::<String>("token")
        .map(String::as_str)
        .unwrap_or("");
    let server = matches.get_one::<String>("server").map(String::as_str);

    if let Err(why) = check_token(token) {
        eprintln!("Bu katılım kodu doğru görünmüyor. Ana hub'daki QR kodunu yeniden okutun.");
        eprintln!("That join code doesn't look right. Please re-scan the QR code on the main hub.");
        if verbose {
            eprintln!("  token: {token} ({why})");
        }
        return 2;
    }
    if verbose {
        eprintln!("  server: {}", server.unwrap_or("<auto-discover>"));
    }
    crate::commands::not_connected::not_connected("join")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_has_token_required() {
        let c = cmd();
        let token = c
            .get_arguments()
            .find(|a| a.get_id().as_str() == "token")
            .expect("token arg");
        assert!(token.is_required_set());
    }

    #[test]
    fn check_token_accepts_the_node_discovery_format() {
        assert!(check_token("ABCDEFGHJKMN").is_ok());
        assert!(check_token(" 23456789ABCD ").is_ok());
    }

    #[test]
    fn check_token_rejects_garbage_and_the_old_placeholder() {
        assert!(check_token("").is_err());
        assert!(check_token("foo").is_err());
        assert!(check_token("ABCDEFGHJKMI").is_err(), "I is outside the charset");
        assert!(check_token("K10::abcdefghijklmnopqrstuvwxyz012345::join-1234567").is_err());
    }
}
