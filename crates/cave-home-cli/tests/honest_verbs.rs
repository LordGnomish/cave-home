// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! No verb may claim a result it never produced.
//!
//! The charter's first-rank user is the non-technical family member: a
//! printed "Acil durum kilitlemesi etkin." (emergency lockdown active) with
//! exit 0 is believed. Every verb that would change or report device / hub
//! state but has no backend wired into the CLI must say so — in Turkish first
//! — print nothing on stdout, and exit with `EXIT_NOT_CONNECTED` (69,
//! sysexits `EX_UNAVAILABLE`). Verbs with a real client are routed to it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::{contains, is_empty};

/// sysexits.h `EX_UNAVAILABLE` — the verb's backend is not connected.
const NOT_CONNECTED: i32 = 69;

/// Every environment variable the CLI reads, removed so tests are hermetic.
const CLI_ENV: &[&str] = &[
    "CAVEHOME_UNIFI_HOST",
    "CAVEHOME_UNIFI_PORT",
    "CAVEHOME_UNIFI_API_KEY",
    "CAVEHOME_UNIFI_USER",
    "CAVEHOME_UNIFI_PASS",
    "CAVEHOME_UNIFI_KIND",
    "CAVEHOME_UNIFI_SITE",
    "CAVEHOME_UNIFI_FINGERPRINT",
    "CAVEHOME_ACCESS_HOST",
    "CAVEHOME_ACCESS_PORT",
    "CAVEHOME_ACCESS_TOKEN",
    "CAVEHOME_ACCESS_FINGERPRINT",
];

fn cavehomectl() -> Command {
    let mut cmd = Command::cargo_bin("cavehomectl").expect("cavehomectl binary");
    for var in CLI_ENV {
        cmd.env_remove(var);
    }
    cmd
}

fn assert_not_connected(args: &[&str]) {
    cavehomectl()
        .args(args)
        .assert()
        .code(NOT_CONNECTED)
        .stdout(is_empty())
        .stderr(contains("henüz bağlı değil"))
        .stderr(contains("not connected"));
}

// ---- the safety-critical UniFi Access verbs -------------------------------

#[test]
fn unifi_access_lockdown_does_not_claim_a_lockdown() {
    assert_not_connected(&["unifi", "access", "lockdown", "--on"]);
    assert_not_connected(&["unifi", "access", "lockdown"]);
}

#[test]
fn unifi_access_unlock_does_not_claim_an_unlock() {
    assert_not_connected(&["unifi", "access", "unlock", "front", "--minutes", "5"]);
}

#[test]
fn unifi_access_status_and_events_do_not_print_canned_data() {
    assert_not_connected(&["unifi", "access", "status"]);
    assert_not_connected(&["unifi", "access", "events"]);
}

#[test]
fn unifi_access_doors_goes_to_the_real_client_not_canned_doors() {
    // Routed to the cave-home-unifi AccessClient (as `unifi live doors`):
    // with no Access host configured it is a config error, never "kilitli".
    cavehomectl()
        .args(["unifi", "access", "doors"])
        .assert()
        .code(2)
        .stdout(is_empty())
        .stderr(contains("CAVEHOME_ACCESS_HOST"));
}

// ---- the rest of the UniFi tree -------------------------------------------

#[test]
fn unifi_network_mutations_are_not_faked() {
    assert_not_connected(&["unifi", "network", "block", "aa:bb:cc:dd:ee:ff"]);
    assert_not_connected(&["unifi", "network", "unblock", "aa:bb:cc:dd:ee:ff"]);
    assert_not_connected(&["unifi", "network", "outlet", "sw1", "3", "--on"]);
    assert_not_connected(&["unifi", "network", "status"]);
}

#[test]
fn unifi_read_verbs_with_a_real_client_are_routed_to_it() {
    for args in [
        ["unifi", "network", "devices"],
        ["unifi", "network", "clients"],
        ["unifi", "protect", "cameras"],
    ] {
        cavehomectl()
            .args(args)
            .assert()
            .code(2)
            .stdout(is_empty())
            .stderr(contains("CAVEHOME_UNIFI_HOST"));
    }
}

#[test]
fn unifi_protect_and_talk_do_not_print_canned_data() {
    assert_not_connected(&["unifi", "protect", "status"]);
    assert_not_connected(&["unifi", "protect", "events"]);
    assert_not_connected(&["unifi", "protect", "seam", "list"]);
    assert_not_connected(&["unifi", "protect", "seam", "assign", "cam1", "native"]);
    assert_not_connected(&["unifi", "talk", "status"]);
    assert_not_connected(&["unifi", "talk", "phones"]);
    assert_not_connected(&["unifi", "talk", "incoming"]);
    assert_not_connected(&["unifi", "talk", "control", "c1", "answer"]);
}

// ---- hub-level verbs ------------------------------------------------------

#[test]
fn status_does_not_say_all_good_without_a_hub() {
    assert_not_connected(&["status"]);
    assert_not_connected(&["--verbose", "status"]);
}

#[test]
fn join_does_not_claim_to_have_joined() {
    // A token in cave-home-node-discovery's JoinToken format is accepted as
    // well-formed, then the CLI says it cannot reach the hub.
    assert_not_connected(&["join", "ABCDEFGHJKMN"]);
}

#[test]
fn join_rejects_the_old_placeholder_token_format() {
    cavehomectl()
        .args(["join", "K10::abcdefghijklmnopqrstuvwxyz012345::join-1234567"])
        .assert()
        .code(2)
        .stdout(is_empty());
}

#[test]
fn device_room_automation_scene_do_not_fake_a_registry() {
    for args in [
        &["device", "list"][..],
        &["device", "add", "--name", "Lamba", "--room", "Salon"],
        &["device", "remove", "Salon lambası"],
        &["room", "list"],
        &["room", "show", "Salon"],
        &["automation", "list"],
        &["automation", "enable", "Tatil modu"],
        &["automation", "disable", "Tatil modu"],
        &["scene", "trigger", "Sleep"],
    ] {
        assert_not_connected(args);
    }
}

#[test]
fn top_does_not_report_an_empty_cluster_it_never_asked() {
    assert_not_connected(&["top", "nodes"]);
    assert_not_connected(&["top", "pods"]);
}

// ---- energy / solar / hue -------------------------------------------------

#[test]
fn energy_never_confirms_a_battery_change_without_the_tesla_transport() {
    assert_not_connected(&["energy", "backup-reserve", "50"]);
    assert_not_connected(&["energy", "mode", "tbc"]);
    assert_not_connected(&["energy", "status"]);
    assert_not_connected(&["energy", "history"]);
    assert_not_connected(&["energy"]);
}

#[test]
fn energy_still_validates_input_first() {
    cavehomectl().args(["energy", "backup-reserve", "150"]).assert().code(1);
    cavehomectl().args(["energy", "mode", "yolo"]).assert().code(1);
}

#[test]
fn solar_does_not_print_demo_production() {
    assert_not_connected(&["solar"]);
    assert_not_connected(&["solar", "status"]);
    assert_not_connected(&["solar", "sunspec", "battery"]);
    assert_not_connected(&["solar", "evcc", "mode", "garage", "pv"]);
    assert_not_connected(&["solar", "forecast", "tomorrow"]);
}

#[test]
fn hue_does_not_fake_lights_or_scenes() {
    assert_not_connected(&["hue", "list-lights"]);
    assert_not_connected(&["hue", "set-scene", "--id", "s1"]);
    assert_not_connected(&["hue", "light", "--id", "l1", "--on", "true"]);
    assert_not_connected(&["hue", "bridge-emu", "enable"]);
    cavehomectl()
        .args(["hue", "set-scene", "--id", "s1", "--action", "disco"])
        .assert()
        .code(1);
}

#[test]
fn jarvis_ask_says_nothing_was_sent() {
    cavehomectl()
        .args(["jarvis", "ask", "--text", "turn on the kitchen light"])
        .assert()
        .success()
        .stdout(contains("gönderilmedi"))
        .stdout(contains("nothing was sent"));
}

// ---- every Phase-1 stub verb ----------------------------------------------

#[test]
fn every_stub_verb_exits_not_connected() {
    for verb in [
        "alarm", "calendar", "camera", "cover", "display", "doorbell", "garden", "history",
        "household", "hvac", "lights", "lock", "matter", "mobile", "music", "notify", "pool",
        "vacuum", "voice", "water", "wellness", "zigbee", "zwave", "esphome", "knx", "free-home",
    ] {
        assert_not_connected(&[verb]);
    }
}

#[test]
fn safety_verbs_with_arguments_exit_not_connected() {
    assert_not_connected(&["lock", "unlock", "--name", "Ön kapı"]);
    assert_not_connected(&["lock", "lock", "--name", "Ön kapı"]);
    assert_not_connected(&["alarm", "disarm"]);
    assert_not_connected(&["alarm", "arm", "--mode", "away"]);
    assert_not_connected(&["cover", "open", "--name", "Garaj"]);
    assert_not_connected(&["lights", "off", "--name", "Salon"]);
}

// ---- verbs that really do their job stay successful -----------------------

#[test]
fn bare_unifi_still_prints_its_usage_summary() {
    cavehomectl().arg("unifi").assert().success().stdout(contains("subcommand required").or(contains("unifi")));
}
