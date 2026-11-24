// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl room {list,show}` — room-centric view of the home.
//!
//! ADR-007 grandma-friendly UX: people think in *rooms* ("turn off
//! everything in the bedroom"), not in entity ids or pod names.
//! `room list` enumerates the rooms cave-home knows about with a
//! per-room device count; `room show <name>` lists every device in
//! one room. Both surfaces stay in home-world vocabulary; technical
//! fields appear only under the global `--verbose` flag (ADR-007 §4).
//!
//! Phase 1 MVP: in-memory demo data shared with `device`; Phase 2b
//! routes through cave-home-apiserver-rs to read the real device
//! registry.

use crate::commands::device;
use clap::{Arg, ArgMatches, Command};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[must_use]
pub fn cmd() -> Command {
    Command::new("room")
        .about("See what's in each room")
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about("Every room cave-home knows about"))
        .subcommand(
            Command::new("show").about("Devices in one room").arg(
                Arg::new("name")
                    .required(true)
                    .help("Room name (e.g. Salon)"),
            ),
        )
}

#[must_use]
pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    let devices = device::demo_devices();
    match matches.subcommand() {
        Some(("list", _)) => {
            print!("{}", render_list(&devices, verbose));
            0
        }
        Some(("show", sub)) => {
            // clap enforces `name` as required; should it ever be absent, say
            // so the way the sibling commands do instead of panicking.
            let Some(name) = sub.get_one::<String>("name") else {
                eprintln!("Internal: missing room name.");
                return 2;
            };
            let (out, code) = render_show(&devices, name, verbose);
            print!("{out}");
            code
        }
        _ => {
            eprintln!("Use 'cavehomectl room --help' to see what's available.");
            2
        }
    }
}

#[must_use]
pub fn render_list(devices: &[device::Device], verbose: bool) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for d in devices {
        *counts.entry(d.room.as_str()).or_insert(0) += 1;
    }
    let mut s = String::from("Your rooms\n----------\n");
    if counts.is_empty() {
        s.push_str("  (no rooms yet — add a device to create one)\n");
        return s;
    }
    for (room, n) in &counts {
        let suffix = if *n == 1 { "device" } else { "devices" };
        let _ = writeln!(s, "  {room} ({n} {suffix})");
    }
    if verbose {
        let _ = writeln!(s, "\n  total rooms: {}", counts.len());
    }
    s
}

#[must_use]
pub fn render_show(devices: &[device::Device], room: &str, verbose: bool) -> (String, i32) {
    let in_room: Vec<&device::Device> = devices
        .iter()
        .filter(|d| d.room.eq_ignore_ascii_case(room))
        .collect();
    if in_room.is_empty() {
        return (
            format!("No room called '{room}'. Try 'cavehomectl room list'.\n"),
            1,
        );
    }
    let mut s = format!("Room: {}\n", in_room[0].room);
    s.push_str(&"-".repeat(6 + in_room[0].room.len()));
    s.push('\n');
    for d in &in_room {
        let _ = writeln!(s, "  {} ({})", d.name, d.kind);
        if verbose {
            let _ = writeln!(s, "    id: {}", d.technical_id);
        }
    }
    (s, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn list_groups_devices_by_room_and_sorts_alphabetically() -> TestResult {
        let out = render_list(&device::demo_devices(), false);
        // Demo data has three rooms: Giriş, Mutfak, Salon — alphabetical.
        let g = out.find("Giriş").ok_or("`Giriş` is not in the room list")?;
        let m = out
            .find("Mutfak")
            .ok_or("`Mutfak` is not in the room list")?;
        let sa = out.find("Salon").ok_or("`Salon` is not in the room list")?;
        assert!(g < m && m < sa, "rooms must render alphabetically");
        assert!(out.contains("1 device"));
        Ok(())
    }

    #[test]
    fn list_empty_shows_friendly_hint() {
        let out = render_list(&[], false);
        assert!(out.contains("no rooms yet"));
    }

    #[test]
    fn list_verbose_appends_total() {
        let out = render_list(&device::demo_devices(), true);
        assert!(out.contains("total rooms: 3"));
    }

    #[test]
    fn show_existing_room_lists_devices() {
        let (out, code) = render_show(&device::demo_devices(), "Mutfak", false);
        assert_eq!(code, 0);
        assert!(out.contains("Mutfak hareket sensörü"));
        assert!(!out.contains("Salon lambası"));
    }

    #[test]
    fn show_is_case_insensitive() {
        let (out, code) = render_show(&device::demo_devices(), "salon", false);
        assert_eq!(code, 0);
        assert!(out.contains("Salon lambası"));
    }

    #[test]
    fn show_verbose_includes_technical_id() {
        let (out, _) = render_show(&device::demo_devices(), "Salon", true);
        assert!(out.contains("0x00158d0003abcdef"));
    }

    #[test]
    fn show_missing_room_returns_exit_1() {
        let (out, code) = render_show(&device::demo_devices(), "Pavyon", false);
        assert_eq!(code, 1);
        assert!(out.contains("No room called 'Pavyon'"));
    }

    #[test]
    fn run_show_without_a_name_exits_two_instead_of_panicking() -> TestResult {
        // `cmd()` makes `name` required, so build the same shape without that
        // rule to reach the branch clap normally rules out.
        let lenient = Command::new("room").subcommand(Command::new("show").arg(Arg::new("name")));
        let matches = lenient.try_get_matches_from(["room", "show"])?;
        assert_eq!(run(&matches, false), 2);
        Ok(())
    }

    #[test]
    fn run_show_with_a_name_reports_the_render_exit_code() {
        let known = cmd().get_matches_from(["room", "show", "Salon"]);
        assert_eq!(run(&known, false), 0);
        let unknown = cmd().get_matches_from(["room", "show", "Pavyon"]);
        assert_eq!(run(&unknown, false), 1);
    }

    #[test]
    fn cmd_advertises_room_subtree() {
        let c = cmd();
        let subs: Vec<_> = c.get_subcommands().map(Command::get_name).collect();
        assert!(subs.contains(&"list"));
        assert!(subs.contains(&"show"));
    }
}
