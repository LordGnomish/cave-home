// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl destroy` — tear down this node's cave-home state.
//!
//! ADR-007 grandma-friendly: the default prompt is plain English /
//! Turkish, and we require explicit confirmation. `--yes` is the
//! scripted escape hatch for the homelabber persona (Charter §2.4).

use clap::{Arg, ArgAction, ArgMatches, Command};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use super::init::default_config_dir;

#[must_use]
pub fn cmd() -> Command {
    Command::new("destroy")
        .about("Remove cave-home from this device (destructive!)")
        .arg(
            Arg::new("yes")
                .long("yes")
                .short('y')
                .help("Skip the confirmation prompt (scripts / homelabber)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("config-dir")
                .long("config-dir")
                .help("Override the config directory")
                .num_args(1),
        )
}

/// Pure helper for the prompt logic, abstracted over IO so it tests
/// without a real TTY.
pub fn confirm<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) -> std::io::Result<bool> {
    writeln!(
        writer,
        "This will erase everything cave-home knows on this device."
    )?;
    write!(writer, "Type DESTROY to confirm: ")?;
    writer.flush()?;
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    Ok(buf.trim() == "DESTROY")
}

/// Actually wipe the config dir. Returns Ok(true) if something was
/// removed.
pub fn wipe(dir: &Path) -> std::io::Result<bool> {
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(dir)?;
    Ok(true)
}

/// Dispatcher entry.
pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    let dir: PathBuf = matches
        .get_one::<String>("config-dir")
        .map(PathBuf::from)
        .unwrap_or_else(default_config_dir);
    let skip_prompt = matches.get_flag("yes");

    if !skip_prompt {
        let stdin = std::io::stdin();
        let mut locked = stdin.lock();
        let mut stdout = std::io::stdout();
        match confirm(&mut locked, &mut stdout) {
            Ok(true) => {}
            Ok(false) => {
                println!("Cancelled. Nothing was removed.");
                return 0;
            }
            Err(e) => {
                eprintln!("Could not read your answer. Cancelled.");
                if verbose {
                    eprintln!("  error: {e}");
                }
                return 1;
            }
        }
    }

    match wipe(&dir) {
        Ok(true) => {
            println!("cave-home has been removed from this device.");
            if verbose {
                println!("  removed: {}", dir.display());
            }
            0
        }
        Ok(false) => {
            println!("Nothing to remove — cave-home isn't set up on this device.");
            0
        }
        Err(e) => {
            eprintln!("Could not remove cave-home. Try running with admin permissions.");
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
    use std::io::Cursor;

    #[test]
    fn confirm_accepts_exact_keyword() {
        let mut reader = Cursor::new(b"DESTROY\n".to_vec());
        let mut writer = Vec::new();
        let ok = confirm(&mut reader, &mut writer).expect("confirm");
        assert!(ok);
    }

    #[test]
    fn confirm_rejects_anything_else() {
        for input in [b"yes\n".to_vec(), b"destroy\n".to_vec(), b"\n".to_vec()] {
            let mut reader = Cursor::new(input);
            let mut writer = Vec::new();
            assert!(!confirm(&mut reader, &mut writer).expect("confirm"));
        }
    }

    #[test]
    fn wipe_missing_dir_is_ok_false() {
        let path = std::env::temp_dir().join("cave-home-destroy-missing-xyz123");
        let _ = std::fs::remove_dir_all(&path);
        assert!(!wipe(&path).expect("wipe"));
    }

    #[test]
    fn wipe_existing_dir_removes_it() {
        let path = std::env::temp_dir().join(format!("cave-home-destroy-{}", std::process::id()));
        std::fs::create_dir_all(&path).expect("mkdir");
        std::fs::write(path.join("marker"), b"x").expect("write");
        assert!(wipe(&path).expect("wipe"));
        assert!(!path.exists());
    }

    #[test]
    fn cmd_name_and_yes_flag() {
        let c = cmd();
        assert_eq!(c.get_name(), "destroy");
        let names: Vec<_> = c
            .get_arguments()
            .map(|a| a.get_id().as_str().to_string())
            .collect();
        assert!(names.contains(&"yes".to_string()));
    }
}
