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

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn cavehomectl() -> Result<Command, Box<dyn std::error::Error>> {
    Ok(Command::cargo_bin("cavehomectl")?)
}

#[test]
fn root_help_lists_every_subcommand() -> TestResult {
    cavehomectl()?
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
    Ok(())
}

#[test]
fn status_default_hides_technical_strings() -> TestResult {
    cavehomectl()?
        .arg("status")
        .assert()
        .success()
        .stdout(contains("cave-home status"))
        .stdout(predicates::str::contains("pod").not())
        .stdout(predicates::str::contains("apiserver").not());
    Ok(())
}

#[test]
fn status_verbose_shows_technical_strings() -> TestResult {
    cavehomectl()?
        .args(["--verbose", "status"])
        .assert()
        .success()
        .stdout(contains("pod"));
    Ok(())
}

#[test]
fn device_list_hides_technical_id_by_default() -> TestResult {
    cavehomectl()?
        .args(["device", "list"])
        .assert()
        .success()
        .stdout(contains("Your devices"))
        .stdout(predicates::str::contains("0x00158d0003abcdef").not());
    Ok(())
}

#[test]
fn device_list_verbose_shows_technical_id() -> TestResult {
    cavehomectl()?
        .args(["--verbose", "device", "list"])
        .assert()
        .success()
        .stdout(contains("0x00158d0003abcdef"));
    Ok(())
}

#[test]
fn automation_list_hides_entity_id() -> TestResult {
    cavehomectl()?
        .args(["automation", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("automation.evening_scene").not());
    Ok(())
}

#[test]
fn room_list_groups_devices_per_room() -> TestResult {
    cavehomectl()?
        .args(["room", "list"])
        .assert()
        .success()
        .stdout(contains("Your rooms"))
        .stdout(contains("Salon"))
        .stdout(contains("device"));
    Ok(())
}

#[test]
fn room_show_returns_devices_in_one_room_case_insensitive() -> TestResult {
    cavehomectl()?
        .args(["room", "show", "salon"])
        .assert()
        .success()
        .stdout(contains("Salon lambası"))
        .stdout(predicates::str::contains("Mutfak hareket sensörü").not());
    Ok(())
}

#[test]
fn room_show_missing_room_exits_one() -> TestResult {
    cavehomectl()?
        .args(["room", "show", "Pavyon"])
        .assert()
        .failure()
        .stdout(contains("No room called"));
    Ok(())
}

#[test]
fn scene_trigger_known_succeeds() -> TestResult {
    cavehomectl()?
        .args(["scene", "trigger", "Sleep"])
        .assert()
        .success()
        .stdout(contains("Running scene"));
    Ok(())
}

#[test]
fn scene_trigger_unknown_fails() -> TestResult {
    cavehomectl()?
        .args(["scene", "trigger", "ghost-scene"])
        .assert()
        .failure();
    Ok(())
}

#[test]
fn join_rejects_garbage_token() -> TestResult {
    cavehomectl()?
        .args(["join", "definitely-not-a-token"])
        .assert()
        .failure();
    Ok(())
}

#[test]
fn destroy_with_yes_and_missing_dir_is_noop() -> TestResult {
    let tmp = std::env::temp_dir().join(format!("cave-home-destroy-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    cavehomectl()?
        .args([
            "destroy",
            "--yes",
            "--config-dir",
            tmp.to_str().ok_or("the temp dir path is not valid UTF-8")?,
        ])
        .assert()
        .success()
        .stdout(contains("Nothing to remove"));
    Ok(())
}

#[test]
fn solar_stub_runs() -> TestResult {
    // F1 will overwrite this command's surface; until then we confirm
    // the stub at least dispatches without panic.
    cavehomectl()?.arg("solar").assert().success();
    Ok(())
}

#[test]
fn unifi_stub_runs() -> TestResult {
    cavehomectl()?.arg("unifi").assert().success();
    Ok(())
}
