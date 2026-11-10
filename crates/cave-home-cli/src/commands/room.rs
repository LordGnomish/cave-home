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

pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    let devices = device::demo_devices();
    match matches.subcommand() {
        Some(("list", _)) => {
            print!("{}", render_list(&devices, verbose));
            0
        }
        Some(("show", sub)) => {
            let name = sub
                .get_one::<String>("name")
                .expect("clap enforces required");
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
        s.push_str(&format!("  {room} ({n} {suffix})\n"));
    }
    if verbose {
        s.push_str(&format!("\n  total rooms: {}\n", counts.len()));
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
        s.push_str(&format!("  {} ({})\n", d.name, d.kind));
        if verbose {
            s.push_str(&format!("    id: {}\n", d.technical_id));
        }
    }
    (s, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_groups_devices_by_room_and_sorts_alphabetically() {
        let out = render_list(&device::demo_devices(), false);
        // Demo data has three rooms: Giriş, Mutfak, Salon — alphabetical.
        let g = out.find("Giriş").expect("Giriş in list");
        let m = out.find("Mutfak").expect("Mutfak in list");
        let sa = out.find("Salon").expect("Salon in list");
        assert!(g < m && m < sa, "rooms must render alphabetically");
        assert!(out.contains("1 device"));
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
    fn cmd_advertises_room_subtree() {
        let c = cmd();
        let subs: Vec<_> = c.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subs.iter().any(|n| *n == "list"));
        assert!(subs.iter().any(|n| *n == "show"));
    }
}
