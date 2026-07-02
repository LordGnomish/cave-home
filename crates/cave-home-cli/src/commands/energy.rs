// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl energy` — Tesla Powerwall / energy-site operations.
//!
//! Subcommands (Charter §6.3 grandma-friendly UX — home-world labels,
//! watts / OAuth / API terminology gated behind `--verbose`):
//!
//! ```text
//!   cavehomectl energy status                       # live power flow + battery
//!   cavehomectl energy mode <self-consumption|backup|tbc>
//!   cavehomectl energy backup-reserve <percent>     # set the outage reserve
//!   cavehomectl energy history --range 24h          # production/use history
//! ```
//!
//! The backend is the `cave-home-tesla` adapter compiled into the one binary
//! (Charter §5); this surface re-uses its `OpMode` parsing so the CLI and the
//! adapter agree on what `tbc` means. Live values shown here are demo data until
//! the adapter's transport is wired (Phase 1b).

use clap::{Arg, ArgMatches, Command};

use cave_home_tesla::OpMode;

/// Build the `energy` clap subtree.
#[must_use]
pub fn cmd() -> Command {
    Command::new("energy")
        .about("Your Powerwall, solar and home battery")
        .subcommand_required(false)
        .subcommand(Command::new("status").about("Show how power is flowing right now"))
        .subcommand(
            Command::new("mode")
                .about("Choose how the battery is used")
                .arg(
                    Arg::new("mode")
                        .required(true)
                        .help("self-consumption | backup | tbc"),
                ),
        )
        .subcommand(
            Command::new("backup-reserve")
                .about("How full to keep the battery for outages")
                .arg(Arg::new("percent").required(true).help("0 to 100")),
        )
        .subcommand(
            Command::new("history")
                .about("Production and use over time")
                .arg(
                    Arg::new("range")
                        .long("range")
                        .default_value("24h")
                        .help("e.g. 24h, 7d"),
                ),
        )
}

/// Entry from `main.rs`. Re-parses argv after the `energy` token so the full
/// subcommand tree works through the simple cross-crate dispatch signature.
#[must_use]
pub fn run() -> i32 {
    let after: Vec<std::ffi::OsString> = std::env::args_os()
        .skip_while(|s| s.to_str() != Some("energy"))
        .collect();
    if after.is_empty() {
        return dispatch(&cmd().get_matches_from(["energy"]), false);
    }
    dispatch(&cmd().get_matches_from(after), false)
}

/// Internal dispatcher — exposed for unit tests.
#[must_use]
pub fn dispatch(matches: &ArgMatches, verbose: bool) -> i32 {
    match matches.subcommand() {
        None | Some(("status", _)) => {
            print!("{}", render_status(&demo_flow(), verbose));
            0
        }
        Some(("mode", m)) => {
            let raw = m.get_one::<String>("mode").map(String::as_str).unwrap_or("");
            match OpMode::from_cli(raw) {
                Some(mode) => {
                    println!("Battery set to: {}.", friendly_mode(mode));
                    0
                }
                None => {
                    eprintln!("I don't know the mode '{raw}'. Try self-consumption, backup or tbc.");
                    1
                }
            }
        }
        Some(("backup-reserve", m)) => {
            let raw = m.get_one::<String>("percent").map(String::as_str).unwrap_or("");
            match raw.parse::<u16>() {
                Ok(p) if p <= 100 => {
                    println!("Keeping the battery at least {p}% full for outages.");
                    0
                }
                _ => {
                    eprintln!("Backup reserve must be a whole number from 0 to 100.");
                    1
                }
            }
        }
        Some(("history", m)) => {
            let range = m.get_one::<String>("range").map(String::as_str).unwrap_or("24h");
            print!("{}", render_history(&demo_history(range), verbose));
            0
        }
        _ => 2,
    }
}

/// A grandma-friendly mode label (English; the Portal renders DE/TR).
fn friendly_mode(mode: OpMode) -> &'static str {
    mode.label(cave_home_tesla::Lang::En)
}

/// ------- render helpers (pure, test-friendly) -----------------------

/// A demo power-flow snapshot, shown until the adapter transport is wired.
#[derive(Debug, Clone, Copy)]
pub struct EnergyFlow {
    /// Solar production, watts.
    pub pv_w: f64,
    /// House load, watts.
    pub load_w: f64,
    /// Grid power, watts (negative = exporting).
    pub grid_w: f64,
    /// State of charge, percent.
    pub soc_pct: f64,
}

/// Demo data for the live flow.
#[must_use]
pub const fn demo_flow() -> EnergyFlow {
    EnergyFlow {
        pv_w: 4200.0,
        load_w: 1800.0,
        grid_w: -900.0,
        soc_pct: 88.0,
    }
}

/// Render the live flow in home-world language.
#[must_use]
pub fn render_status(f: &EnergyFlow, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("Energy right now\n");
    out.push_str("================\n");
    out.push_str(&format!("  Solar making     {:.1} kW\n", f.pv_w / 1000.0));
    out.push_str(&format!("  Home using       {:.1} kW\n", f.load_w / 1000.0));
    out.push_str(&format!("  Battery          {:.0}% full\n", f.soc_pct));
    if f.grid_w < 0.0 {
        out.push_str(&format!(
            "  Sending to grid  {:.1} kW (earning credits)\n",
            -f.grid_w / 1000.0
        ));
    } else {
        out.push_str(&format!("  Taking from grid {:.1} kW\n", f.grid_w / 1000.0));
    }
    if verbose {
        out.push_str("\n[developer] raw fields:\n");
        out.push_str(&format!(
            "  pv_w={} load_w={} grid_w={} soc_pct={}\n",
            f.pv_w as i64, f.load_w as i64, f.grid_w as i64, f.soc_pct as i64
        ));
    }
    out
}

/// A demo history summary for a range token.
#[derive(Debug, Clone)]
pub struct EnergyHistory {
    /// The requested range token (e.g. `24h`).
    pub range: String,
    /// Produced energy, kWh.
    pub produced_kwh: f64,
    /// Consumed energy, kWh.
    pub used_kwh: f64,
}

/// Demo history for `range`.
#[must_use]
pub fn demo_history(range: &str) -> EnergyHistory {
    EnergyHistory {
        range: range.to_string(),
        produced_kwh: 38.4,
        used_kwh: 22.1,
    }
}

/// Render the history summary.
#[must_use]
pub fn render_history(h: &EnergyHistory, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("Energy history\n");
    out.push_str("==============\n");
    out.push_str(&format!("  Over the last  {}\n", h.range));
    out.push_str(&format!("  Made           {:.1} kWh of sunshine\n", h.produced_kwh));
    out.push_str(&format!("  Used           {:.1} kWh at home\n", h.used_kwh));
    if verbose {
        out.push_str(&format!(
            "\n[developer] produced_kwh={:.1} used_kwh={:.1}\n",
            h.produced_kwh, h.used_kwh
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_name_is_energy() {
        assert_eq!(cmd().get_name(), "energy");
    }

    #[test]
    fn cmd_has_all_subcommands() {
        let names: Vec<_> = cmd().get_subcommands().map(|s| s.get_name().to_string()).collect();
        for required in ["status", "mode", "backup-reserve", "history"] {
            assert!(names.iter().any(|n| n == required), "missing energy sub: {required}");
        }
    }

    #[test]
    fn dispatch_status_exits_zero() {
        let m = cmd().get_matches_from(["energy", "status"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_mode_accepts_tbc() {
        let m = cmd().get_matches_from(["energy", "mode", "tbc"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_mode_accepts_self_consumption() {
        let m = cmd().get_matches_from(["energy", "mode", "self-consumption"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_mode_rejects_unknown() {
        let m = cmd().get_matches_from(["energy", "mode", "yolo"]);
        assert_eq!(dispatch(&m, false), 1);
    }

    #[test]
    fn dispatch_backup_reserve_accepts_valid() {
        let m = cmd().get_matches_from(["energy", "backup-reserve", "50"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_backup_reserve_rejects_over_100() {
        let m = cmd().get_matches_from(["energy", "backup-reserve", "150"]);
        assert_eq!(dispatch(&m, false), 1);
    }

    #[test]
    fn dispatch_backup_reserve_rejects_non_number() {
        let m = cmd().get_matches_from(["energy", "backup-reserve", "lots"]);
        assert_eq!(dispatch(&m, false), 1);
    }

    #[test]
    fn dispatch_history_exits_zero() {
        let m = cmd().get_matches_from(["energy", "history", "--range", "24h"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn render_status_default_hides_jargon() {
        let out = render_status(&demo_flow(), false);
        for forbidden in ["OAuth", "Fleet", "instant_power", "self_consumption", "watts"] {
            assert!(!out.contains(forbidden), "leaked '{forbidden}': {out}");
        }
        assert!(out.contains("Solar"));
        assert!(out.contains("Battery"));
    }

    #[test]
    fn render_status_verbose_shows_raw_watts() {
        let out = render_status(&demo_flow(), true);
        assert!(out.contains("pv_w="));
        assert!(out.contains("soc_pct="));
    }
}
