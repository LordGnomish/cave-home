// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! Validates the shipped macOS LaunchAgent plist: it must be well-formed and
//! actually schedule the daily `poll && measure && report` run at 06:00.

use std::path::PathBuf;
use std::process::Command;

fn plist_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dist/com.gnomish.cave-home-tracker.plist")
}

fn metrics_plist_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("dist/com.gnomish.cave-home-tracker-metrics.plist")
}

#[test]
fn metrics_plist_serves_prometheus_on_9102() {
    let path = metrics_plist_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));

    assert!(text.starts_with("<?xml"), "must be an XML plist");
    assert!(text.contains("<string>com.gnomish.cave-home-tracker-metrics</string>"));
    // A long-lived, auto-started metrics service.
    assert!(text.contains("<key>KeepAlive</key>"));
    assert!(text.contains("<key>RunAtLoad</key>"));
    // Binds the Prometheus endpoint on :9102 via `dashboard --addr`.
    assert!(text.contains("<string>dashboard</string>"));
    assert!(text.contains("127.0.0.1:9102"));

    if let Ok(output) = Command::new("plutil").arg("-lint").arg(&path).output() {
        assert!(
            output.status.success(),
            "plutil -lint failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn plist_exists_and_declares_the_daily_run() {
    let path = plist_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));

    assert!(text.starts_with("<?xml"), "must be an XML plist");
    assert!(text.contains("<key>Label</key>"));
    assert!(text.contains("<string>com.gnomish.cave-home-tracker</string>"));

    // Scheduled daily at 06:00.
    assert!(text.contains("<key>StartCalendarInterval</key>"));
    assert!(text.contains("<key>Hour</key>"));
    assert!(text.contains("<integer>6</integer>"));
    assert!(text.contains("<key>Minute</key>"));
    assert!(text.contains("<integer>0</integer>"));

    // Runs poll, then measure, then report (order matters: && chain).
    let cmd_line = text
        .lines()
        .find(|l| l.contains("cave-home-tracker") && l.contains("poll"))
        .expect("a ProgramArguments command line invoking the tracker");
    let poll = cmd_line.find("poll").unwrap();
    let measure = cmd_line.find("measure").unwrap();
    let report = cmd_line.find("report").unwrap();
    assert!(
        poll < measure && measure < report,
        "poll -> measure -> report order"
    );
}

/// When `plutil` is present (macOS), the plist must lint clean.
#[test]
fn plist_passes_plutil_lint_if_available() {
    let path = plist_path();
    let Ok(output) = Command::new("plutil").arg("-lint").arg(&path).output() else {
        eprintln!("plutil not available; skipping lint");
        return;
    };
    assert!(
        output.status.success(),
        "plutil -lint failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
