// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl join <token>` — join a running cluster.
//!
//! ADR-005 §a: the CLI is the power-user join surface; the same
//! primitive sits behind the OS image's first-boot wizard and the
//! Portal "Add node" wizard. ADR-007: the raw token is technical;
//! the user-visible output stays grandma-friendly.

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

/// Lightweight validation that the token shape is plausible. Real
/// crypto verification happens server-side in cave-home-cluster.
///
/// Token format (placeholder, Phase 2b will tighten):
/// `K10::<32+ ascii base64-ish chars>::<10+ chars>`
#[must_use]
pub fn token_is_well_formed(token: &str) -> bool {
    if !token.starts_with("K10::") {
        return false;
    }
    let parts: Vec<&str> = token.split("::").collect();
    parts.len() == 3 && parts[1].len() >= 32 && parts[2].len() >= 10
}

/// Dispatcher entry. The CLI just validates shape and prints a
/// grandma-friendly progress message — actual cluster wire-up will
/// route through `cave-home-cluster` once that crate exposes a
/// public `join()` API (Phase 2b).
pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    let token = matches
        .get_one::<String>("token")
        .map(String::as_str)
        .unwrap_or("");
    let server = matches.get_one::<String>("server").map(String::as_str);

    if !token_is_well_formed(token) {
        eprintln!("That join code doesn't look right. Please re-scan the QR code on the main hub.");
        if verbose {
            eprintln!("  token: {token}");
        }
        return 2;
    }

    println!("Joining your cave-home hub...");
    if verbose {
        println!("  token: {token}");
        if let Some(s) = server {
            println!("  server: {s}");
        } else {
            println!("  server: <auto-discover>");
        }
    }
    // Phase 2b: call cave-home-cluster::join(token, server)
    println!("Connected. This hub is now part of your cave-home.");
    0
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
    fn token_well_formed_accepts_plausible_shape() {
        let t = "K10::abcdefghijklmnopqrstuvwxyz012345::join-1234567";
        assert!(token_is_well_formed(t));
    }

    #[test]
    fn token_well_formed_rejects_garbage() {
        assert!(!token_is_well_formed(""));
        assert!(!token_is_well_formed("foo"));
        assert!(!token_is_well_formed("K10::short::ok"));
        assert!(!token_is_well_formed("K10::abcdefghijklmnopqrstuvwxyz012345"));
    }
}
