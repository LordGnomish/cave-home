// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl status` — overall cluster / node / service health.
//!
//! ADR-007 mandates we render this in home-world vocabulary by
//! default. Pod / node / kubelet names live behind `--verbose`.
//!
//! The CLI has no connection to the hub's health sources yet, so `status`
//! says so and exits `EXIT_NOT_CONNECTED` — it never prints "All good" for a
//! home it did not look at. [`render`] (tested on fixture rows) is what the
//! live gatherer will feed.

use clap::{Arg, ArgAction, ArgMatches, Command};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Ok,
    Warn,
    Down,
}

impl Health {
    /// Grandma-friendly label.
    #[must_use]
    pub const fn friendly(self) -> &'static str {
        match self {
            Self::Ok => "All good",
            Self::Warn => "Needs attention",
            Self::Down => "Offline",
        }
    }

    /// Technical label, used under `--verbose`.
    #[must_use]
    pub const fn technical(self) -> &'static str {
        match self {
            Self::Ok => "Ready",
            Self::Warn => "Degraded",
            Self::Down => "NotReady",
        }
    }
}

/// One row in the status report.
pub struct StatusRow {
    pub label: &'static str,
    pub health: Health,
    /// Technical fact (e.g. K3s pod name) — only printed under `--verbose`.
    pub technical: String,
}

/// Build the clap subtree.
#[must_use]
pub fn cmd() -> Command {
    Command::new("status")
        .about("Show how your cave-home is doing")
        .arg(
            Arg::new("watch")
                .long("watch")
                .short('w')
                .help("Re-print every 2s (press Ctrl-C to stop)")
                .action(ArgAction::SetTrue),
        )
}

/// Fixture rows for the [`render`] tests — **not** shown to the user; the
/// live gatherer (Phase 2b, via `cave-home-apiserver-rs`) replaces it.
#[must_use]
pub fn gather_demo_status() -> Vec<StatusRow> {
    vec![
        StatusRow {
            label: "Hub",
            health: Health::Ok,
            technical: "apiserver=Ready, kine=Ready".into(),
        },
        StatusRow {
            label: "Devices",
            health: Health::Ok,
            technical: "zigbee2mqtt-pod=Ready, zwave-pod=Ready".into(),
        },
        StatusRow {
            label: "Cameras",
            health: Health::Warn,
            technical: "frigate-pod=Degraded (1 stream timing out)".into(),
        },
        StatusRow {
            label: "Voice",
            health: Health::Ok,
            technical: "whisper-pod=Ready, piper-pod=Ready".into(),
        },
        StatusRow {
            label: "Solar",
            health: Health::Ok,
            technical: "evcc-pod=Ready, sunspec-pod=Ready".into(),
        },
    ]
}

/// Render the report. Returns the string instead of printing so tests
/// can assert on it.
#[must_use]
pub fn render(rows: &[StatusRow], verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("cave-home status\n");
    out.push_str("================\n");
    for r in rows {
        let label = if verbose { r.health.technical() } else { r.health.friendly() };
        out.push_str(&format!("  {:<12}  {}\n", r.label, label));
        if verbose {
            out.push_str(&format!("                  {}\n", r.technical));
        }
    }
    out
}

/// Dispatcher entry. No health source is connected: report that, never a
/// made-up "All good".
#[must_use]
pub fn run(matches: &ArgMatches, _verbose: bool) -> i32 {
    let _watch = matches.get_flag("watch");
    crate::commands::not_connected::not_connected("status")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_labels_never_leak_tech_terms() {
        for h in [Health::Ok, Health::Warn, Health::Down] {
            let f = h.friendly();
            for forbidden in ["pod", "kubelet", "apiserver", "etcd", "NotReady", "Ready"] {
                assert!(
                    !f.contains(forbidden),
                    "friendly label '{f}' leaked '{forbidden}' (ADR-007)"
                );
            }
        }
    }

    #[test]
    fn render_default_hides_technical_strings() {
        let rows = gather_demo_status();
        let out = render(&rows, false);
        assert!(!out.contains("pod"));
        assert!(!out.contains("kine"));
        assert!(!out.contains("apiserver"));
        assert!(out.contains("Hub"));
        assert!(out.contains("All good"));
    }

    #[test]
    fn render_verbose_shows_technical_strings() {
        let rows = gather_demo_status();
        let out = render(&rows, true);
        assert!(out.contains("pod"));
        assert!(out.contains("apiserver"));
    }

    #[test]
    fn cmd_name() {
        assert_eq!(cmd().get_name(), "status");
    }
}
