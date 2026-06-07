// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl solar` — solar / EV / battery / forecast operations.
//!
//! Subcommands (Charter §6.3 grandma-friendly UX — home-world labels,
//! Modbus / kW / SunSpec terminology gated behind `--verbose`):
//!
//! ```text
//!   cavehomectl solar status                  # overall summary
//!   cavehomectl solar sunspec                 # inverter + battery
//!   cavehomectl solar evcc list               # loadpoints
//!   cavehomectl solar evcc mode <lp> <mode>   # set loadpoint mode
//!   cavehomectl solar forecast today          # today's forecast
//!   cavehomectl solar forecast tomorrow       # tomorrow's forecast
//! ```

use clap::{Arg, ArgMatches, Command};

/// Build the `solar` clap subtree.
#[must_use]
pub fn cmd() -> Command {
    Command::new("solar")
        .about("Solar, EV charging, battery and weather forecast")
        .subcommand_required(false)
        .subcommand(Command::new("status").about("Show how much solar you're producing right now"))
        .subcommand(
            Command::new("sunspec")
                .about("Solar inverter & home battery readings")
                .subcommand(Command::new("inverter").about("Solar inverter snapshot"))
                .subcommand(Command::new("battery").about("Home battery snapshot")),
        )
        .subcommand(
            Command::new("evcc")
                .about("EV chargers, heat-pump and surplus loop")
                .subcommand(Command::new("list").about("List loadpoints and their modes"))
                .subcommand(
                    Command::new("mode")
                        .about("Set charge mode for a loadpoint")
                        .arg(Arg::new("loadpoint").required(true))
                        .arg(
                            Arg::new("mode")
                                .required(true)
                                .help("off | now | minpv | pv"),
                        ),
                ),
        )
        .subcommand(
            Command::new("forecast")
                .about("Solar production forecast")
                .subcommand(Command::new("today").about("Today's expected solar production"))
                .subcommand(Command::new("tomorrow").about("Tomorrow's expected solar production")),
        )
}

/// Entry from `main.rs`. The F5 contract uses the simpler signature
/// `() -> i32` for cross-agent stubs; for solar we re-parse argv here
/// so we can honour `cavehomectl solar status`, etc. without changing
/// the F5 dispatch signature.
#[must_use]
pub fn run() -> i32 {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    // Skip the binary name and the leading "solar" token.
    let after_solar: Vec<std::ffi::OsString> = argv
        .into_iter()
        .skip_while(|s| s.to_str() != Some("solar"))
        .collect();
    if after_solar.is_empty() {
        // Default to status when called outside the normal dispatcher.
        let m = cmd().get_matches_from(["solar"]);
        return dispatch(&m, false);
    }
    let matches = cmd().get_matches_from(after_solar);
    dispatch(&matches, false)
}

/// Internal dispatcher — exposed for unit tests.
pub fn dispatch(matches: &ArgMatches, verbose: bool) -> i32 {
    match matches.subcommand() {
        None | Some(("status", _)) => {
            print!("{}", render_status_summary(&demo_summary(), verbose));
            0
        }
        Some(("sunspec", sub)) => match sub.subcommand() {
            None | Some(("inverter", _)) => {
                print!("{}", render_inverter(&demo_inverter(), verbose));
                0
            }
            Some(("battery", _)) => {
                print!("{}", render_battery(&demo_battery(), verbose));
                0
            }
            _ => 2,
        },
        Some(("evcc", sub)) => match sub.subcommand() {
            None | Some(("list", _)) => {
                print!("{}", render_loadpoints(&demo_loadpoints(), verbose));
                0
            }
            Some(("mode", m)) => {
                let lp = m
                    .get_one::<String>("loadpoint")
                    .map(String::as_str)
                    .unwrap_or("");
                let mode = m
                    .get_one::<String>("mode")
                    .map(String::as_str)
                    .unwrap_or("");
                if !["off", "now", "minpv", "pv"].contains(&mode) {
                    eprintln!("Unknown mode '{mode}'. Use off, now, minpv or pv.");
                    return 1;
                }
                println!("Set {lp} to {mode}.");
                0
            }
            _ => 2,
        },
        Some(("forecast", sub)) => match sub.subcommand() {
            None | Some(("today", _)) => {
                print!("{}", render_forecast(&demo_forecast_today(), verbose));
                0
            }
            Some(("tomorrow", _)) => {
                print!("{}", render_forecast(&demo_forecast_tomorrow(), verbose));
                0
            }
            _ => 2,
        },
        _ => 2,
    }
}

/// ------- render helpers (pure, test-friendly) -----------------------

#[derive(Debug, Clone, Copy)]
pub struct SolarSummary {
    pub solar_kw: f64,
    pub home_kw: f64,
    pub battery_soc: f64,
    pub grid_kw: f64,
}

#[must_use]
pub fn demo_summary() -> SolarSummary {
    SolarSummary {
        solar_kw: 5.2,
        home_kw: 1.4,
        battery_soc: 72.0,
        grid_kw: -3.8,
    }
}

#[must_use]
pub fn render_status_summary(s: &SolarSummary, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("Solar status\n");
    out.push_str("=============\n");
    out.push_str(&format!("  Solar producing  {:.1} kW\n", s.solar_kw));
    out.push_str(&format!("  Home using       {:.1} kW\n", s.home_kw));
    out.push_str(&format!("  Battery          {:.0}% full\n", s.battery_soc));
    if s.grid_kw < 0.0 {
        out.push_str(&format!(
            "  Sending to grid  {:.1} kW (earning credits)\n",
            -s.grid_kw
        ));
    } else {
        out.push_str(&format!("  Taking from grid {:.1} kW\n", s.grid_kw));
    }
    if verbose {
        out.push_str("\n[developer] raw fields:\n");
        out.push_str(&format!(
            "  grid_w={} pv_w={} battery_soc_pct={}\n",
            (s.grid_kw * 1000.0) as i32,
            (s.solar_kw * 1000.0) as i32,
            s.battery_soc as i32,
        ));
    }
    out
}

#[derive(Debug, Clone)]
pub struct InverterSnapshot {
    pub manufacturer: String,
    pub model: String,
    pub ac_power_kw: f64,
    pub dc_voltage_v: f64,
    pub status: String,
}

#[must_use]
pub fn demo_inverter() -> InverterSnapshot {
    InverterSnapshot {
        manufacturer: "Fronius".into(),
        model: "Symo 8.2-3-M".into(),
        ac_power_kw: 5.2,
        dc_voltage_v: 412.0,
        status: "producing".into(),
    }
}

#[must_use]
pub fn render_inverter(i: &InverterSnapshot, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("Solar inverter\n");
    out.push_str("==============\n");
    out.push_str(&format!("  Currently        {}\n", i.status));
    out.push_str(&format!("  Producing        {:.1} kW\n", i.ac_power_kw));
    if verbose {
        out.push_str("\n[developer] raw fields:\n");
        out.push_str(&format!("  manufacturer  : {}\n", i.manufacturer));
        out.push_str(&format!("  model         : {}\n", i.model));
        out.push_str(&format!("  dc_voltage_v  : {:.1}\n", i.dc_voltage_v));
    }
    out
}

#[derive(Debug, Clone, Copy)]
pub struct BatterySnapshot {
    pub soc_percent: f64,
    pub power_kw: f64,
    pub status: &'static str,
}

#[must_use]
pub const fn demo_battery() -> BatterySnapshot {
    BatterySnapshot {
        soc_percent: 72.0,
        power_kw: 1.4,
        status: "charging",
    }
}

#[must_use]
pub fn render_battery(b: &BatterySnapshot, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("Home battery\n");
    out.push_str("============\n");
    out.push_str(&format!("  Status   {}\n", b.status));
    out.push_str(&format!("  Battery  {:.0}% full\n", b.soc_percent));
    if verbose {
        out.push_str(&format!("\n[developer] power_kw={:.2}\n", b.power_kw));
    }
    out
}

#[derive(Debug, Clone)]
pub struct LoadpointRow {
    pub name: String,
    pub kind: &'static str,
    pub mode: &'static str,
    pub current_a: u16,
}

#[must_use]
pub fn demo_loadpoints() -> Vec<LoadpointRow> {
    vec![
        LoadpointRow {
            name: "Garage wallbox".into(),
            kind: "EV charger",
            mode: "pv",
            current_a: 12,
        },
        LoadpointRow {
            name: "Heat pump".into(),
            kind: "Heat pump",
            mode: "minpv",
            current_a: 8,
        },
    ]
}

#[must_use]
pub fn render_loadpoints(rows: &[LoadpointRow], verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("EV chargers & heat-pump\n");
    out.push_str("=======================\n");
    for r in rows {
        out.push_str(&format!("  {:<20}  {}  mode={}\n", r.name, r.kind, r.mode));
        if verbose {
            out.push_str(&format!("    [developer] current_a={}\n", r.current_a));
        }
    }
    out
}

#[derive(Debug, Clone, Copy)]
pub struct ForecastRow {
    pub kwh_today: f64,
    pub kwh_tomorrow: f64,
    pub peak_kw: f64,
    pub source: &'static str,
}

#[must_use]
pub const fn demo_forecast_today() -> ForecastRow {
    ForecastRow {
        kwh_today: 41.2,
        kwh_tomorrow: 39.1,
        peak_kw: 7.4,
        source: "forecast.solar",
    }
}

#[must_use]
pub const fn demo_forecast_tomorrow() -> ForecastRow {
    ForecastRow {
        kwh_today: 41.2,
        kwh_tomorrow: 39.1,
        peak_kw: 7.0,
        source: "forecast.solar",
    }
}

#[must_use]
pub fn render_forecast(f: &ForecastRow, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("Solar forecast\n");
    out.push_str("==============\n");
    out.push_str(&format!("  Today      {:.1} kWh expected\n", f.kwh_today));
    out.push_str(&format!(
        "  Tomorrow   {:.1} kWh expected\n",
        f.kwh_tomorrow
    ));
    out.push_str(&format!("  Peak hour  {:.1} kW\n", f.peak_kw));
    if verbose {
        out.push_str(&format!("\n[developer] source={}\n", f.source));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_name_is_solar() {
        assert_eq!(cmd().get_name(), "solar");
    }

    #[test]
    fn cmd_has_all_subcommands() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name()).collect();
        for required in ["status", "sunspec", "evcc", "forecast"] {
            assert!(names.contains(&required), "missing solar sub: {required}");
        }
    }

    #[test]
    fn dispatch_status_default_exits_zero() {
        let m = cmd().get_matches_from(["solar", "status"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_evcc_list_exits_zero() {
        let m = cmd().get_matches_from(["solar", "evcc", "list"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_evcc_mode_rejects_unknown() {
        let m = cmd().get_matches_from(["solar", "evcc", "mode", "lp1", "yolo"]);
        assert_eq!(dispatch(&m, false), 1);
    }

    #[test]
    fn dispatch_evcc_mode_accepts_pv() {
        let m = cmd().get_matches_from(["solar", "evcc", "mode", "lp1", "pv"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_sunspec_battery_exits_zero() {
        let m = cmd().get_matches_from(["solar", "sunspec", "battery"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_forecast_today_exits_zero() {
        let m = cmd().get_matches_from(["solar", "forecast", "today"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn render_status_default_never_leaks_tech() {
        let s = demo_summary();
        let out = render_status_summary(&s, false);
        for forbidden in ["Modbus", "register", "K3s", "kine", "pod", "Watt ", "evcc"] {
            assert!(!out.contains(forbidden), "leaked '{forbidden}': {out}");
        }
        assert!(out.contains("Solar"));
        assert!(out.contains("Battery"));
    }

    #[test]
    fn render_status_verbose_shows_raw_fields() {
        let s = demo_summary();
        let out = render_status_summary(&s, true);
        assert!(out.contains("grid_w="));
        assert!(out.contains("pv_w="));
    }

    #[test]
    fn render_inverter_default_hides_manufacturer() {
        let i = demo_inverter();
        let out = render_inverter(&i, false);
        assert!(!out.contains("Fronius"));
        assert!(out.contains("producing"));
    }

    #[test]
    fn render_inverter_verbose_shows_manufacturer() {
        let i = demo_inverter();
        let out = render_inverter(&i, true);
        assert!(out.contains("Fronius"));
        assert!(out.contains("manufacturer"));
    }

    #[test]
    fn render_battery_default_omits_power_w() {
        let b = demo_battery();
        let out = render_battery(&b, false);
        assert!(!out.contains("power_kw"));
    }

    #[test]
    fn render_loadpoints_uses_home_world_kind() {
        let rows = demo_loadpoints();
        let out = render_loadpoints(&rows, false);
        assert!(out.contains("EV charger") || out.contains("Heat pump"));
        assert!(!out.contains("loadpoint"));
        assert!(!out.contains("EVCC"));
    }

    #[test]
    fn render_forecast_default_omits_source() {
        let f = demo_forecast_today();
        let out = render_forecast(&f, false);
        assert!(!out.contains("forecast.solar"));
        assert!(out.contains("Today"));
        assert!(out.contains("Peak"));
    }
}
