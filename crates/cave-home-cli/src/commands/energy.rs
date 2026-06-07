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
