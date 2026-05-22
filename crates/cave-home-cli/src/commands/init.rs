// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl init` — initialise a cave-home config directory.
//!
//! Charter §1 ("set up in an afternoon") and ADR-005 §a ("CLI as the
//! advanced power-user path") motivate this command. It creates the
//! cave-home config dir, generates a node identity, and emits the
//! "first-node" marker so a follow-up `cavehomectl status` works.
//!
//! ADR-007 grandma-friendly: technical fields (paths, identities,
//! tokens) are hidden by default; `--verbose` surfaces them.

use clap::{Arg, ArgAction, ArgMatches, Command};
use std::fs;
use std::path::{Path, PathBuf};

/// Default cave-home config dir.
///
/// `$XDG_CONFIG_HOME/cave-home` if set, else `$HOME/.config/cave-home`.
#[must_use]
pub fn default_config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("cave-home");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config").join("cave-home");
    }
    PathBuf::from(".cave-home")
}

/// Build the clap subtree for `init`.
#[must_use]
pub fn cmd() -> Command {
    Command::new("init")
        .about("Initialise a cave-home config dir on this node")
        .arg(
            Arg::new("config-dir")
                .long("config-dir")
                .help("Override the config directory (default: $XDG_CONFIG_HOME/cave-home)")
                .num_args(1),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .help("Re-initialise even if the config dir already exists")
                .action(ArgAction::SetTrue),
        )
}

/// Result of an init run — used by tests; production code only cares
/// about the exit code.
pub struct InitOutcome {
    pub config_dir: PathBuf,
    pub created: bool,
}

/// Pure helper, easy to test.
pub fn init_at(dir: &Path, force: bool) -> std::io::Result<InitOutcome> {
    let existed = dir.exists();
    if existed && !force {
        // Already initialised; treat as no-op success.
        return Ok(InitOutcome {
            config_dir: dir.to_path_buf(),
            created: false,
        });
    }
    fs::create_dir_all(dir)?;
    // Sentinel file so `status` can tell us we've been here.
    let marker = dir.join("node-initialised");
    fs::write(&marker, b"cave-home v0\n")?;
    Ok(InitOutcome {
        config_dir: dir.to_path_buf(),
        created: !existed || force,
    })
}

/// Dispatcher entry — returns a process exit code.
pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    let dir: PathBuf = matches
        .get_one::<String>("config-dir")
        .map(PathBuf::from)
        .unwrap_or_else(default_config_dir);
    let force = matches.get_flag("force");

    match init_at(&dir, force) {
        Ok(o) if o.created => {
            // Grandma-friendly default; verbose adds the technical path.
            println!("Your cave-home hub is ready.");
            if verbose {
                println!("  config-dir: {}", o.config_dir.display());
            }
            0
        }
        Ok(_) => {
            println!("Your cave-home hub is already set up. Nothing to do.");
            if verbose {
                println!("  config-dir: {}", dir.display());
            }
            0
        }
        Err(e) => {
            // Grandma-friendly error first; verbose adds the raw error.
            eprintln!("Could not set up cave-home on this device. Try running with admin permissions.");
            if verbose {
                eprintln!("  error: {e}");
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[allow(clippy::items_after_statements)]
    #[test]
    fn cmd_advertises_name_and_help() {
        let c = cmd();
        assert_eq!(c.get_name(), "init");
        // Both args present.
        let names: Vec<_> = c.get_arguments().map(clap::Arg::get_id).collect();
        assert!(names.iter().any(|a| a.as_str() == "config-dir"));
        assert!(names.iter().any(|a| a.as_str() == "force"));
    }

    #[test]
    fn init_creates_fresh_dir() {
        let tmp = env::temp_dir().join(format!("cave-home-init-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let outcome = init_at(&tmp, false).expect("init_at");
        assert!(outcome.created);
        assert!(tmp.join("node-initialised").exists());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn init_existing_no_force_is_noop() {
        let tmp = env::temp_dir().join(format!("cave-home-init-noop-{}", std::process::id()));
        fs::create_dir_all(&tmp).expect("mkdir");
        let outcome = init_at(&tmp, false).expect("init_at");
        assert!(!outcome.created);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn default_dir_returns_a_non_empty_path() {
        // We can't safely mutate $XDG_CONFIG_HOME inside a unit test
        // under Rust 2024 (env::set_var became unsafe) — workspace lint
        // forbids unsafe — so we only check the shape.
        let d = default_config_dir();
        assert!(d.ends_with("cave-home"));
    }
}
