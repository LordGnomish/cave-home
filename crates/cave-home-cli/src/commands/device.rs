// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl device {add,list,remove}` — device CRUD surface.
//!
//! Phase 1 MVP: in-memory ops + grandma-friendly output. Phase 2b
//! will route to cave-home-apiserver-rs via the `apiserver`
//! sibling module.

use clap::{Arg, ArgMatches, Command};

/// One device row, home-world vocabulary only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub room: String,
    pub kind: String,
    /// Internal id (MAC/IEEE/EUI64) — only printed under `--verbose`.
    pub technical_id: String,
}

#[must_use]
pub fn demo_devices() -> Vec<Device> {
    vec![
        Device {
            name: "Salon lambası".into(),
            room: "Salon".into(),
            kind: "light".into(),
            technical_id: "0x00158d0003abcdef".into(),
        },
        Device {
            name: "Mutfak hareket sensörü".into(),
            room: "Mutfak".into(),
            kind: "motion".into(),
            technical_id: "0x00158d0003123456".into(),
        },
        Device {
            name: "Ön kapı kilidi".into(),
            room: "Giriş".into(),
            kind: "lock".into(),
            technical_id: "ZW-node-7".into(),
        },
    ]
}

#[must_use]
pub fn render_list(devices: &[Device], verbose: bool) -> String {
    let mut s = String::new();
    s.push_str("Your devices\n");
    s.push_str("------------\n");
    for d in devices {
        s.push_str(&format!("  {} ({}, {})\n", d.name, d.room, d.kind));
        if verbose {
            s.push_str(&format!("    id: {}\n", d.technical_id));
        }
    }
    s
}

#[must_use]
pub fn cmd() -> Command {
    Command::new("device")
        .about("Manage devices in your cave-home")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about("Show every device cave-home knows about"))
        .subcommand(
            Command::new("add")
                .about("Add a new device (scans for 5s by default)")
                .arg(
                    Arg::new("name")
                        .long("name")
                        .help("Friendly name to give this device")
                        .num_args(1),
                )
                .arg(
                    Arg::new("room")
                        .long("room")
                        .help("Room this device belongs to")
                        .num_args(1),
                ),
        )
        .subcommand(
            Command::new("remove").about("Forget a device").arg(
                Arg::new("name")
                    .help("Device name to remove (use `device list` first)")
                    .required(true)
                    .num_args(1),
            ),
        )
}

/// Dispatcher entry.
pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    match matches.subcommand() {
        Some(("list", _sub)) => {
            print!("{}", render_list(&demo_devices(), verbose));
            0
        }
        Some(("add", sub)) => {
            let name = sub
                .get_one::<String>("name")
                .cloned()
                .unwrap_or_else(|| "(auto-named)".to_string());
            let room = sub
                .get_one::<String>("room")
                .cloned()
                .unwrap_or_else(|| "(unassigned)".to_string());
            println!("Searching for a new device... (5 seconds)");
            // Phase 2b: actually scan Zigbee / Z-Wave / Matter.
            println!("Added '{name}' to room '{room}'.");
            0
        }
        Some(("remove", sub)) => {
            // SAFETY: name is required, clap guarantees it.
            let Some(name) = sub.get_one::<String>("name") else {
                eprintln!("Internal: missing device name.");
                return 2;
            };
            println!("Forgot device '{name}'.");
            0
        }
        _ => {
            eprintln!("Use one of: list, add, remove");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_has_three_subcommands() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name()).collect();
        assert!(names.contains(&"list"));
        assert!(names.contains(&"add"));
        assert!(names.contains(&"remove"));
    }

    #[test]
    fn render_list_hides_technical_id_by_default() {
        let out = render_list(&demo_devices(), false);
        assert!(!out.contains("0x00158d0003abcdef"));
        assert!(!out.contains("ZW-node-7"));
        assert!(out.contains("Salon lambası"));
    }

    #[test]
    fn render_list_shows_id_under_verbose() {
        let out = render_list(&demo_devices(), true);
        assert!(out.contains("0x00158d0003abcdef"));
        assert!(out.contains("ZW-node-7"));
    }
}
