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

/// `--verbose` is a global flag, so `solar`, `energy`, `hue` and `jarvis`
/// have to honour it the way `status` and `device` do, wherever it is written.
///
/// Runs `view` plain (no developer line) and then with `--verbose` and `-v`,
/// before and after it, expecting `developer_line` every time. The expected
/// lines are the ones the render unit tests in `src/commands/` pin.
fn assert_verbose_adds(view: &[&str], developer_line: &str) -> TestResult {
    cavehomectl()?
        .args(view)
        .assert()
        .success()
        .stdout(contains("[developer]").not());
    for flag in ["--verbose", "-v"] {
        cavehomectl()?
            .arg(flag)
            .args(view)
            .assert()
            .success()
            .stdout(contains(developer_line));
        cavehomectl()?
            .args(view)
            .arg(flag)
            .assert()
            .success()
            .stdout(contains(developer_line));
    }
    Ok(())
}

#[test]
fn solar_verbose_shows_raw_fields() -> TestResult {
    let raw = "grid_w=-3800 pv_w=5200 battery_soc_pct=72";
    assert_verbose_adds(&["solar"], raw)?;
    assert_verbose_adds(&["solar", "status"], raw)?;
    assert_verbose_adds(&["solar", "sunspec", "inverter"], "manufacturer  : Fronius")?;
    assert_verbose_adds(
        &["solar", "sunspec", "battery"],
        "[developer] power_kw=1.40",
    )?;
    assert_verbose_adds(&["solar", "evcc", "list"], "[developer] current_a=12")?;
    let source = "[developer] source=forecast.solar";
    assert_verbose_adds(&["solar", "forecast", "today"], source)?;
    assert_verbose_adds(&["solar", "forecast", "tomorrow"], source)?;
    Ok(())
}

#[test]
fn energy_verbose_shows_raw_fields() -> TestResult {
    let raw = "pv_w=4200 load_w=1800 grid_w=-900 soc_pct=88";
    assert_verbose_adds(&["energy"], raw)?;
    assert_verbose_adds(&["energy", "status"], raw)?;
    assert_verbose_adds(
        &["energy", "history"],
        "[developer] produced_kwh=38.4 used_kwh=22.1",
    )?;
    Ok(())
}

#[test]
fn hue_list_lights_verbose_shows_raw_colour_points() -> TestResult {
    assert_verbose_adds(
        &["hue", "list-lights"],
        "Living room xy=(0.450,0.410) bri=203",
    )
}

#[test]
fn jarvis_verbose_shows_tool_names() -> TestResult {
    assert_verbose_adds(&["jarvis"], "[developer] set_light")?;
    assert_verbose_adds(&["jarvis", "tools"], "[developer] set_light")?;
    assert_verbose_adds(
        &["jarvis", "ask", "--text", "turn the kitchen light on"],
        "[developer] tool=set_light args={\"on\":true,\"target\":\"kitchen\"}",
    )?;
    Ok(())
}

#[test]
fn verbose_is_accepted_by_views_without_technical_fields() -> TestResult {
    // Nothing technical to add here, so the output stays what it is without
    // the flag — but the flag must not be rejected, in either position.
    for view in [
        &["solar", "evcc", "mode", "garage", "pv"][..],
        &["energy", "mode", "backup"],
        &["energy", "backup-reserve", "20"],
        &["hue", "set-scene", "--id", "abc"],
        &["hue", "status"],
        &["jarvis", "wake"],
    ] {
        let plain = cavehomectl()?.args(view).assert().success();
        let expected = plain.get_output().stdout.clone();
        for flag in ["--verbose", "-v"] {
            cavehomectl()?
                .arg(flag)
                .args(view)
                .assert()
                .success()
                .stdout(expected.clone());
            cavehomectl()?
                .args(view)
                .arg(flag)
                .assert()
                .success()
                .stdout(expected.clone());
        }
    }
    Ok(())
}
