// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl` — the cave-home command-line tool.
//!
//! ADR-005 §2: the CLI is the **advanced power-user path**; the
//! OS image and Portal "Add node" wizard call the same primitives
//! under the covers. ADR-007: home-world vocabulary by default,
//! `--verbose` for raw fields.

use clap::{Arg, ArgAction, Command};

use cave_home_cli::commands::{
    automation, destroy, device, free_home, hue, init, join, knx, scene, solar, status, unifi,
};

fn build_cli() -> Command {
    Command::new("cavehomectl")
        .about("Control your cave-home from the command line")
        .version(env!("CARGO_PKG_VERSION"))
        .arg_required_else_help(true)
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .global(true)
                .help("Show technical fields (paths, ids, pod names) — ADR-007 escape hatch")
                .action(ArgAction::SetTrue),
        )
        .subcommand(init::cmd())
        .subcommand(join::cmd())
        .subcommand(status::cmd())
        .subcommand(destroy::cmd())
        .subcommand(device::cmd())
        .subcommand(automation::cmd())
        .subcommand(scene::cmd())
        // Cross-agent stubs — F1-F4 fill these:
        .subcommand(solar::cmd())
        .subcommand(unifi::cmd())
        .subcommand(hue::cmd())
        .subcommand(knx::cmd())
        .subcommand(free_home::cmd())
}

fn main() {
    std::process::exit(run_with_args(std::env::args_os()));
}

/// Test seam: dispatch given an explicit arg iterator. Returns the
/// exit code instead of calling `std::process::exit`.
pub fn run_with_args<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let matches = build_cli().get_matches_from(args);
    let verbose = matches.get_flag("verbose");

    match matches.subcommand() {
        Some(("init", sub)) => init::run(sub, verbose),
        Some(("join", sub)) => join::run(sub, verbose),
        Some(("status", sub)) => status::run(sub, verbose),
        Some(("destroy", sub)) => destroy::run(sub, verbose),
        Some(("device", sub)) => device::run(sub, verbose),
        Some(("automation", sub)) => automation::run(sub, verbose),
        Some(("scene", sub)) => scene::run(sub, verbose),
        // Cross-agent stubs use the simpler signature.
        Some(("solar", _)) => solar::run(),
        Some(("unifi", _)) => unifi::run(),
        Some(("hue", _)) => hue::run(),
        Some(("knx", _)) => knx::run(),
        Some(("free-home", _)) => free_home::run(),
        _ => {
            eprintln!("Use 'cavehomectl --help' to see what's available.");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_lists_every_subcommand() {
        let cli = build_cli();
        let names: Vec<_> = cli.get_subcommands().map(|s| s.get_name()).collect();
        for required in [
            "init",
            "join",
            "status",
            "destroy",
            "device",
            "automation",
            "scene",
            "solar",
            "unifi",
            "hue",
            "knx",
            "free-home",
        ] {
            assert!(
                names.iter().any(|n| *n == required),
                "missing subcommand '{required}' in CLI"
            );
        }
    }

    #[test]
    fn verbose_is_global() {
        let cli = build_cli();
        let v = cli
            .get_arguments()
            .find(|a| a.get_id().as_str() == "verbose")
            .expect("verbose flag");
        assert!(v.is_global_set());
    }

    #[test]
    fn dispatch_status_exits_zero() {
        let code = run_with_args(["cavehomectl", "status"].iter().map(|s| s.to_string()));
        // Demo data has one Warn but no Down, so exit 0.
        assert_eq!(code, 0);
    }

    #[test]
    fn dispatch_unknown_top_level_args() {
        // clap will exit non-zero on parse error; we don't try to
        // intercept that here — instead, confirm a known-good path
        // returns 0.
        let code = run_with_args(["cavehomectl", "device", "list"].iter().map(|s| s.to_string()));
        assert_eq!(code, 0);
    }

    #[test]
    fn dispatch_scene_trigger_known() {
        let code = run_with_args(
            ["cavehomectl", "scene", "trigger", "Sleep"]
                .iter()
                .map(|s| s.to_string()),
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn dispatch_scene_trigger_unknown() {
        let code = run_with_args(
            ["cavehomectl", "scene", "trigger", "nonexistent-scene"]
                .iter()
                .map(|s| s.to_string()),
        );
        assert_eq!(code, 1);
    }
}
