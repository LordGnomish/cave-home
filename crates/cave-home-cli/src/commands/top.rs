// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl top nodes` / `cavehomectl top pods` — show live node / pod CPU
//! and memory usage, the `kubectl top` equivalent backed by the in-process
//! `metrics_server` pipeline (ADR-004).
//!
//! The table layout lives in [`crate::top`]. This module is the clap wiring.
//! Until the in-process metrics runtime is attached to the CLI process
//! (ADR-004 phase-1b), the live row source is empty, so the commands render the
//! honest `No resources found` rather than fabricating usage.

use clap::{ArgMatches, Command};

use crate::top::{NodeTopRow, PodTopRow, render_top_nodes, render_top_pods};

#[must_use]
pub fn cmd() -> Command {
    Command::new("top")
        .about("Show live node / device-group CPU and memory usage")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("nodes").about("CPU / memory usage per cluster node"))
        .subcommand(Command::new("pods").about("CPU / memory usage per workload (device group)"))
}

/// The live node rows. Phase 1b attaches the in-process metrics pipeline; until
/// then there is no source and the slice is empty.
#[must_use]
pub const fn live_node_rows() -> Vec<NodeTopRow> {
    Vec::new()
}

/// The live pod rows. Empty until the metrics pipeline is attached (phase 1b).
#[must_use]
pub const fn live_pod_rows() -> Vec<PodTopRow> {
    Vec::new()
}

pub fn run(matches: &ArgMatches, _verbose: bool) -> i32 {
    match matches.subcommand() {
        Some(("nodes", _)) => {
            println!("{}", render_top_nodes(&live_node_rows()));
            0
        }
        Some(("pods", _)) => {
            println!("{}", render_top_pods(&live_pod_rows(), true));
            0
        }
        _ => {
            eprintln!("Use: top nodes | top pods");
            2
        }
    }
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
    fn live_rows_are_empty_until_phase_1b() {
        assert!(live_node_rows().is_empty());
        assert!(live_pod_rows().is_empty());
    }
}
