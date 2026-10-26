// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl scene trigger <name>` — run a scene.
//!
//! No scene engine is connected to the CLI yet, so `trigger` exits
//! `EXIT_NOT_CONNECTED` and runs nothing. [`resolve`] (case-insensitive name
//! match) is the pure piece the live path will reuse.

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

/// Fixture scene names for the resolver tests — never shown as real.
#[must_use]
pub fn demo_scenes() -> &'static [&'static str] {
    &["Akşam", "Romantic Dinner", "Movie Night", "Wake Up", "Sleep"]
}

/// Dispatcher entry: no scene engine is connected — say so, run nothing.
#[must_use]
pub fn run(matches: &ArgMatches, _verbose: bool) -> i32 {
    let Some(("trigger", _)) = matches.subcommand() else {
        eprintln!("Use: scene trigger <name>");
        return 2;
    };
    crate::commands::not_connected::not_connected("scene trigger")
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
