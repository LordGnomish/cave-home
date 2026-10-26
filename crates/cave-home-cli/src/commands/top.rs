// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl top nodes` / `cavehomectl top pods` — show live node / pod CPU
//! and memory usage, the `kubectl top` equivalent backed by the in-process
//! `metrics_server` pipeline (ADR-004).
//!
//! The table layout lives in [`crate::top`]. This module is the clap wiring.
//! The in-process metrics runtime is not attached to the CLI process yet
//! (ADR-004 phase-1b), so both verbs exit `EXIT_NOT_CONNECTED` — not "No
//! resources found", which would claim the cluster was asked and is empty.

use clap::{ArgMatches, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("top")
        .about("Show live node / device-group CPU and memory usage")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("nodes").about("CPU / memory usage per cluster node"))
        .subcommand(Command::new("pods").about("CPU / memory usage per workload (device group)"))
}

/// Dispatcher entry: no metrics source is attached — say so.
#[must_use]
pub fn run(matches: &ArgMatches, _verbose: bool) -> i32 {
    let what = match matches.subcommand() {
        Some(("nodes", _)) => "top nodes",
        Some(("pods", _)) => "top pods",
        _ => {
            eprintln!("Use: top nodes | top pods");
            return 2;
        }
    };
    crate::commands::not_connected::not_connected(what)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_requires_a_subcommand() {
        // `top` with no sub-command errors (arg_required_else_help).
        assert!(cmd().try_get_matches_from(["top"]).is_err());
    }

    #[test]
    fn nodes_and_pods_parse() {
        assert!(cmd().try_get_matches_from(["top", "nodes"]).is_ok());
        assert!(cmd().try_get_matches_from(["top", "pods"]).is_ok());
    }

    #[test]
    fn run_reports_not_connected() {
        let m = cmd().try_get_matches_from(["top", "nodes"]).unwrap();
        assert_eq!(run(&m, false), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
