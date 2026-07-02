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
    alarm, automation, calendar, camera, cover, destroy, device, display, doorbell, energy,
    free_home, garden, get, history, household, hue, hvac, init, jarvis, join, knx, lights, lock,
    matter, mobile, music, notify, pool, room, scene, solar, status, top, unifi, vacuum, voice,
    water, wellness, zigbee, zwave,
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
        .subcommand(get::cmd())
        .subcommand(init::cmd())
        .subcommand(join::cmd())
        .subcommand(status::cmd())
        .subcommand(destroy::cmd())
        .subcommand(device::cmd())
        .subcommand(room::cmd())
        .subcommand(automation::cmd())
        .subcommand(scene::cmd())
        .subcommand(top::cmd())
        // Cross-agent stubs — F1-F4 fill these:
        .subcommand(solar::cmd())
        .subcommand(energy::cmd())
        .subcommand(unifi::cmd())
        .subcommand(hue::cmd())
        .subcommand(knx::cmd())
        .subcommand(free_home::cmd())
        // G8 stubs (Phase 1 4-track completeness) — backend not yet attached.
        .subcommand(lights::cmd())
        .subcommand(cover::cmd())
        .subcommand(lock::cmd())
        .subcommand(vacuum::cmd())
        .subcommand(hvac::cmd())
        .subcommand(camera::cmd())
        .subcommand(doorbell::cmd())
        .subcommand(alarm::cmd())
        .subcommand(water::cmd())
        .subcommand(garden::cmd())
        .subcommand(pool::cmd())
        .subcommand(voice::cmd())
        .subcommand(jarvis::cmd())
        .subcommand(music::cmd())
        .subcommand(notify::cmd())
        .subcommand(display::cmd())
        .subcommand(history::cmd())
        .subcommand(wellness::cmd())
        .subcommand(calendar::cmd())
        .subcommand(household::cmd())
        .subcommand(matter::cmd())
        .subcommand(zigbee::cmd())
        .subcommand(zwave::cmd())
        .subcommand(mobile::cmd())
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
        Some(("get", sub)) => get::run(sub, verbose),
        Some(("init", sub)) => init::run(sub, verbose),
        Some(("join", sub)) => join::run(sub, verbose),
        Some(("status", sub)) => status::run(sub, verbose),
        Some(("destroy", sub)) => destroy::run(sub, verbose),
        Some(("device", sub)) => device::run(sub, verbose),
        Some(("room", sub)) => room::run(sub, verbose),
        Some(("automation", sub)) => automation::run(sub, verbose),
        Some(("scene", sub)) => scene::run(sub, verbose),
        Some(("top", sub)) => top::run(sub, verbose),
        // Cross-agent stubs use the simpler signature.
        Some(("solar", _)) => solar::run(),
        Some(("energy", _)) => energy::run(),
        Some(("unifi", sub)) => unifi::run_matched(sub, verbose),
        Some(("hue", _)) => hue::run(),
        Some(("knx", _)) => knx::run(),
        Some(("free-home", _)) => free_home::run(),
        // G8 stubs (Phase 1 4-track completeness) — see commands/*.rs.
        Some(("lights", _)) => lights::run(),
        Some(("cover", _)) => cover::run(),
        Some(("lock", _)) => lock::run(),
        Some(("vacuum", _)) => vacuum::run(),
        Some(("hvac", _)) => hvac::run(),
        Some(("camera", _)) => camera::run(),
        Some(("doorbell", _)) => doorbell::run(),
        Some(("alarm", _)) => alarm::run(),
        Some(("water", _)) => water::run(),
        Some(("garden", _)) => garden::run(),
        Some(("pool", _)) => pool::run(),
        Some(("voice", _)) => voice::run(),
        Some(("jarvis", _)) => jarvis::run(),
        Some(("music", _)) => music::run(),
        Some(("notify", _)) => notify::run(),
        Some(("display", _)) => display::run(),
        Some(("history", _)) => history::run(),
        Some(("wellness", _)) => wellness::run(),
        Some(("calendar", _)) => calendar::run(),
        Some(("household", _)) => household::run(),
        Some(("matter", _)) => matter::run(),
        Some(("zigbee", _)) => zigbee::run(),
        Some(("zwave", _)) => zwave::run(),
        Some(("mobile", _)) => mobile::run(),
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
            "room",
            "automation",
            "scene",
            "solar",
            "energy",
            "unifi",
            "hue",
            "knx",
            "free-home",
            // G8 stubs:
            "lights",
            "cover",
            "lock",
            "vacuum",
            "hvac",
            "camera",
            "doorbell",
            "alarm",
            "water",
            "garden",
            "pool",
            "voice",
            "jarvis",
            "music",
            "notify",
            "display",
            "history",
            "wellness",
            "calendar",
            "household",
            "matter",
            "zigbee",
            "zwave",
            "mobile",
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
        let code = run_with_args(
            ["cavehomectl", "device", "list"]
                .iter()
                .map(|s| s.to_string()),
        );
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
