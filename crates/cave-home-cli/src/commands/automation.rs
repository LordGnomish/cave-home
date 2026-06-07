// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl automation {list,enable,disable}` — automation surface.

use clap::{Arg, ArgMatches, Command};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Automation {
    pub name: String,
    pub enabled: bool,
    /// Technical id (HA-style entity id) — verbose-only.
    pub technical_id: String,
}

#[must_use]
pub fn demo_automations() -> Vec<Automation> {
    vec![
        Automation {
            name: "Akşam senaryosu".into(),
            enabled: true,
            technical_id: "automation.evening_scene".into(),
        },
        Automation {
            name: "Kimse yoksa lambaları kapat".into(),
            enabled: true,
            technical_id: "automation.lights_off_when_empty".into(),
        },
        Automation {
            name: "Tatil modu".into(),
            enabled: false,
            technical_id: "automation.vacation_mode".into(),
        },
    ]
}

#[must_use]
pub fn render_list(items: &[Automation], verbose: bool) -> String {
    let mut s = String::new();
    s.push_str("Your automations\n");
    s.push_str("----------------\n");
    for a in items {
        let mark = if a.enabled { "[ON] " } else { "[OFF]" };
        s.push_str(&format!("  {mark} {}\n", a.name));
        if verbose {
            s.push_str(&format!("        id: {}\n", a.technical_id));
        }
    }
    s
}

#[must_use]
pub fn cmd() -> Command {
    Command::new("automation")
        .about("Manage the automations that make your home do things")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about("Show every automation"))
        .subcommand(
            Command::new("enable")
                .about("Turn an automation on")
                .arg(
                    Arg::new("name")
                        .help("Automation name (from `automation list`)")
                        .required(true)
                        .num_args(1),
                ),
        )
        .subcommand(
            Command::new("disable")
                .about("Turn an automation off")
                .arg(
                    Arg::new("name")
                        .help("Automation name")
                        .required(true)
                        .num_args(1),
                ),
        )
}

pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    match matches.subcommand() {
        Some(("list", _)) => {
            print!("{}", render_list(&demo_automations(), verbose));
            0
        }
        Some(("enable", sub)) => {
            let Some(name) = sub.get_one::<String>("name") else {
                eprintln!("Internal: missing automation name.");
                return 2;
            };
            println!("'{name}' is now ON.");
            0
        }
        Some(("disable", sub)) => {
            let Some(name) = sub.get_one::<String>("name") else {
                eprintln!("Internal: missing automation name.");
                return 2;
            };
            println!("'{name}' is now OFF.");
            0
        }
        _ => {
            eprintln!("Use one of: list, enable, disable");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_subcommands() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name()).collect();
        assert!(names.contains(&"list"));
        assert!(names.contains(&"enable"));
        assert!(names.contains(&"disable"));
    }

    #[test]
    fn render_hides_entity_id_by_default() {
        let out = render_list(&demo_automations(), false);
        assert!(!out.contains("automation.evening_scene"));
        assert!(out.contains("Akşam senaryosu"));
    }

    #[test]
    fn render_shows_entity_id_under_verbose() {
        let out = render_list(&demo_automations(), true);
        assert!(out.contains("automation.evening_scene"));
    }
}
