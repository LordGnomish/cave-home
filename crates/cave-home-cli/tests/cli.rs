// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Integration tests for the `cavehomectl` binary.
//!
//! Charter Golden Rule #1 (adapted for first-party crates): every
//! command surface has a `--help` example test, so the help text
//! itself is checked into CI.

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
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
fn status_default_hides_technical_strings() {
    cavehomectl()
        .arg("status")
        .assert()
        .success()
        .stdout(contains("cave-home status"))
        .stdout(predicates::str::contains("pod").not())
        .stdout(predicates::str::contains("apiserver").not());
}

#[test]
fn status_verbose_shows_technical_strings() {
    cavehomectl()
        .args(["--verbose", "status"])
        .assert()
        .success()
        .stdout(contains("pod"));
}

#[test]
fn device_list_hides_technical_id_by_default() {
    cavehomectl()
        .args(["device", "list"])
        .assert()
        .success()
        .stdout(contains("Your devices"))
        .stdout(predicates::str::contains("0x00158d0003abcdef").not());
}

#[test]
fn device_list_verbose_shows_technical_id() {
    cavehomectl()
        .args(["--verbose", "device", "list"])
        .assert()
        .success()
        .stdout(contains("0x00158d0003abcdef"));
}

#[test]
fn automation_list_hides_entity_id() {
    cavehomectl()
        .args(["automation", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("automation.evening_scene").not());
}

#[test]
fn room_list_groups_devices_per_room() {
    cavehomectl()
        .args(["room", "list"])
        .assert()
        .success()
        .stdout(contains("Your rooms"))
        .stdout(contains("Salon"))
        .stdout(contains("device"));
}

#[test]
fn room_show_returns_devices_in_one_room_case_insensitive() {
    cavehomectl()
        .args(["room", "show", "salon"])
        .assert()
        .success()
        .stdout(contains("Salon lambası"))
        .stdout(predicates::str::contains("Mutfak hareket sensörü").not());
}

#[test]
fn room_show_missing_room_exits_one() {
    cavehomectl()
        .args(["room", "show", "Pavyon"])
        .assert()
        .failure()
        .stdout(contains("No room called"));
}

#[test]
fn scene_trigger_known_succeeds() {
    cavehomectl()
        .args(["scene", "trigger", "Sleep"])
        .assert()
        .success()
        .stdout(contains("Running scene"));
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
    let tmp = std::env::temp_dir().join(format!("cave-home-destroy-test-{}", std::process::id()));
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
fn solar_stub_runs() {
    // F1 will overwrite this command's surface; until then we confirm
    // the stub at least dispatches without panic.
    cavehomectl().arg("solar").assert().success();
}

#[test]
fn unifi_stub_runs() {
    cavehomectl().arg("unifi").assert().success();
}
