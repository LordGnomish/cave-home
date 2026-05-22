// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl scene trigger <name>` — run a scene.

use clap::{Arg, ArgMatches, Command};

#[must_use]
pub fn cmd() -> Command {
    Command::new("scene")
        .about("Trigger a saved scene (e.g. 'Akşam', 'Romantic Dinner')")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("trigger")
                .about("Trigger a scene by name")
                .arg(
                    Arg::new("name")
                        .help("Scene name (case-insensitive)")
                        .required(true)
                        .num_args(1),
                ),
        )
}

/// Pure helper for tests: given a list of known scenes and a query,
/// returns the best match (case-insensitive exact), if any.
#[must_use]
pub fn resolve<'a>(scenes: &'a [&'a str], query: &str) -> Option<&'a str> {
    scenes
        .iter()
        .copied()
        .find(|s| s.eq_ignore_ascii_case(query))
}

#[must_use]
pub fn demo_scenes() -> &'static [&'static str] {
    &["Akşam", "Romantic Dinner", "Movie Night", "Wake Up", "Sleep"]
}

pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    let Some(("trigger", sub)) = matches.subcommand() else {
        eprintln!("Use: scene trigger <name>");
        return 2;
    };
    let Some(name) = sub.get_one::<String>("name") else {
        eprintln!("Internal: missing scene name.");
        return 2;
    };
    match resolve(demo_scenes(), name) {
        Some(matched) => {
            println!("Running scene: {matched}");
            if verbose {
                println!("  resolved from query: {name:?}");
            }
            0
        }
        None => {
            eprintln!(
                "No scene called '{name}'. Try one of: {}.",
                demo_scenes().join(", ")
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_requires_trigger_subcommand() {
        let c = cmd();
        assert_eq!(c.get_name(), "scene");
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name()).collect();
        assert!(names.contains(&"trigger"));
    }

    #[test]
    fn resolve_is_case_insensitive() {
        let scenes = ["Akşam", "Movie Night"];
        let scenes: Vec<&str> = scenes.to_vec();
        assert_eq!(resolve(&scenes, "akşam"), Some("Akşam"));
        assert_eq!(resolve(&scenes, "MOVIE NIGHT"), Some("Movie Night"));
        assert_eq!(resolve(&scenes, "nope"), None);
    }
}
