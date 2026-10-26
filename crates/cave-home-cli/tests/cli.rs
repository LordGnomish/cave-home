// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Integration tests for the `cavehomectl` binary.
//!
//! Charter Golden Rule #1 (adapted for first-party crates): every
//! command surface has a `--help` example test, so the help text
//! itself is checked into CI.
//!
//! Verbs without a wired backend are covered by `honest_verbs.rs`: they exit
//! `EXIT_NOT_CONNECTED` instead of printing demo data (the ADR-007 rendering
//! rules are unit-tested on the render helpers in each command module).

use assert_cmd::Command;
use predicates::str::contains;

fn cavehomectl() -> Command {
    Command::cargo_bin("cavehomectl").expect("cavehomectl binary")
}

#[test]
fn root_help_lists_every_subcommand() {
    cavehomectl()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("init"))
        .stdout(contains("join"))
        .stdout(contains("status"))
        .stdout(contains("destroy"))
        .stdout(contains("device"))
        .stdout(contains("room"))
        .stdout(contains("automation"))
        .stdout(contains("scene"))
        .stdout(contains("solar"))
        .stdout(contains("unifi"))
        .stdout(contains("hue"))
        .stdout(contains("knx"))
        .stdout(contains("free-home"));
}

#[test]
fn scene_trigger_unknown_fails() {
    cavehomectl()
        .args(["scene", "trigger", "ghost-scene"])
        .assert()
        .failure();
}

#[test]
fn join_rejects_garbage_token() {
    cavehomectl()
        .args(["join", "definitely-not-a-token"])
        .assert()
        .failure();
}

#[test]
fn destroy_with_yes_and_missing_dir_is_noop() {
    let tmp = std::env::temp_dir().join(format!(
        "cave-home-destroy-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    cavehomectl()
        .args([
            "destroy",
            "--yes",
            "--config-dir",
            tmp.to_str().expect("path"),
        ])
        .assert()
        .success()
        .stdout(contains("Nothing to remove"));
}

#[test]
fn unifi_stub_runs() {
    cavehomectl().arg("unifi").assert().success();
}
