// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §3.8 (CSA public PDF) only; Z2M source NOT consulted.
//! OnOff cluster (0x0006) — ZCL §3.8.
//!
//! RED: references `cave_home_zigbee::onoff` (`OnOffCommand`, `OnOffState`,
//! `StartUpOnOff`, `command_id`, `attribute_id`, `ON_OFF_CLUSTER_ID`) which
//! do not exist yet. Hand-computed wire vectors are derived directly from
//! the ZCL §3.8 command tables.

use cave_home_zigbee::onoff::{
    attribute_id, command_id, OnOffCommand, OnOffState, StartUpOnOff, ON_OFF_CLUSTER_ID,
};

#[test]
fn cluster_and_command_ids_match_spec() {
    // ZCL §3.8.1: OnOff cluster id is 0x0006.
    assert_eq!(ON_OFF_CLUSTER_ID, 0x0006);
    // ZCL §3.8.2.3 received-command table.
    assert_eq!(command_id::OFF, 0x00);
    assert_eq!(command_id::ON, 0x01);
    assert_eq!(command_id::TOGGLE, 0x02);
    assert_eq!(command_id::OFF_WITH_EFFECT, 0x40);
    assert_eq!(command_id::ON_WITH_RECALL_GLOBAL_SCENE, 0x41);
    assert_eq!(command_id::ON_WITH_TIMED_OFF, 0x42);
}

#[test]
fn attribute_ids_match_spec() {
    // ZCL §3.8.2.2 attribute table.
    assert_eq!(attribute_id::ON_OFF, 0x0000);
    assert_eq!(attribute_id::GLOBAL_SCENE_CONTROL, 0x4000);
    assert_eq!(attribute_id::ON_TIME, 0x4001);
    assert_eq!(attribute_id::OFF_WAIT_TIME, 0x4002);
    assert_eq!(attribute_id::START_UP_ON_OFF, 0x4003);
}

#[test]
fn parse_payloadless_commands() {
    assert_eq!(
        OnOffCommand::parse(command_id::OFF, &[]).unwrap(),
        OnOffCommand::Off
    );
    assert_eq!(
        OnOffCommand::parse(command_id::ON, &[]).unwrap(),
        OnOffCommand::On
    );
    assert_eq!(
        OnOffCommand::parse(command_id::TOGGLE, &[]).unwrap(),
        OnOffCommand::Toggle
    );
    assert_eq!(
        OnOffCommand::parse(command_id::ON_WITH_RECALL_GLOBAL_SCENE, &[]).unwrap(),
        OnOffCommand::OnWithRecallGlobalScene
    );
}

#[test]
fn parse_off_with_effect() {
    // §3.8.2.3.4: effect identifier (u8) + effect variant (u8).
    let cmd = OnOffCommand::parse(command_id::OFF_WITH_EFFECT, &[0x00, 0x01]).unwrap();
    assert_eq!(
        cmd,
        OnOffCommand::OffWithEffect {
            effect_id: 0x00,
            effect_variant: 0x01,
        }
    );
}

#[test]
fn parse_on_with_timed_off() {
    // §3.8.2.3.6: on_off_control (u8) + on_time (u16 LE) + off_wait_time (u16 LE).
    // on_time = 0x012c = 300 (30 s), off_wait_time = 0x0064 = 100 (10 s).
    let cmd =
        OnOffCommand::parse(command_id::ON_WITH_TIMED_OFF, &[0x00, 0x2c, 0x01, 0x64, 0x00]).unwrap();
    assert_eq!(
        cmd,
        OnOffCommand::OnWithTimedOff {
            on_off_control: 0x00,
            on_time: 300,
            off_wait_time: 100,
        }
    );
}

#[test]
fn parse_rejects_truncated_payloads() {
    assert!(OnOffCommand::parse(command_id::OFF_WITH_EFFECT, &[0x00]).is_err());
    assert!(OnOffCommand::parse(command_id::ON_WITH_TIMED_OFF, &[0x00, 0x2c, 0x01]).is_err());
}

#[test]
fn parse_rejects_unknown_command() {
    assert!(OnOffCommand::parse(0x7f, &[]).is_err());
}

#[test]
fn command_id_round_trips() {
    for cmd in [
        OnOffCommand::Off,
        OnOffCommand::On,
        OnOffCommand::Toggle,
        OnOffCommand::OnWithRecallGlobalScene,
        OnOffCommand::OffWithEffect {
            effect_id: 1,
            effect_variant: 2,
        },
        OnOffCommand::OnWithTimedOff {
            on_off_control: 1,
            on_time: 5,
            off_wait_time: 7,
        },
    ] {
        let id = cmd.command_id();
        let payload = cmd.encode_payload();
        assert_eq!(OnOffCommand::parse(id, &payload).unwrap(), cmd);
    }
}

#[test]
fn state_defaults_to_off() {
    let s = OnOffState::new();
    assert!(!s.on);
    assert_eq!(s.start_up, StartUpOnOff::Previous);
}

#[test]
fn apply_on_off_toggle() {
    let mut s = OnOffState::new();
    s.apply(&OnOffCommand::On);
    assert!(s.on);
    // §3.8.2.2.2: On sets GlobalSceneControl to TRUE.
    assert!(s.global_scene_control);
    s.apply(&OnOffCommand::Off);
    assert!(!s.on);
    s.apply(&OnOffCommand::Toggle);
    assert!(s.on);
    s.apply(&OnOffCommand::Toggle);
    assert!(!s.on);
}

#[test]
fn apply_off_with_effect_turns_off() {
    let mut s = OnOffState::new();
    s.apply(&OnOffCommand::On);
    s.apply(&OnOffCommand::OffWithEffect {
        effect_id: 0,
        effect_variant: 0,
    });
    assert!(!s.on);
}

#[test]
fn apply_on_with_timed_off_sets_timers() {
    let mut s = OnOffState::new();
    s.apply(&OnOffCommand::OnWithTimedOff {
        on_off_control: 0x00,
        on_time: 300,
        off_wait_time: 100,
    });
    assert!(s.on);
    assert_eq!(s.on_time, 300);
    assert_eq!(s.off_wait_time, 100);
}

#[test]
fn timed_off_accept_only_when_on_is_ignored_while_off() {
    // §3.8.2.3.6.1: bit 0 (accept_only_when_on) set + currently off ⇒ ignore.
    let mut s = OnOffState::new();
    assert!(!s.on);
    s.apply(&OnOffCommand::OnWithTimedOff {
        on_off_control: 0x01,
        on_time: 300,
        off_wait_time: 100,
    });
    assert!(!s.on, "command must be ignored when off and accept_only_when_on set");
}

#[test]
fn start_up_on_off_round_trips() {
    // §3.8.2.2.5 StartUpOnOff enum: 0=Off, 1=On, 2=Toggle, 0xff=Previous.
    for (v, e) in [
        (0x00u8, StartUpOnOff::Off),
        (0x01, StartUpOnOff::On),
        (0x02, StartUpOnOff::Toggle),
        (0xff, StartUpOnOff::Previous),
    ] {
        assert_eq!(StartUpOnOff::from_u8(v).unwrap(), e);
        assert_eq!(e.to_u8(), v);
    }
    assert!(StartUpOnOff::from_u8(0x03).is_err());
}

#[test]
fn power_on_applies_start_up_behavior() {
    // StartUpOnOff::On ⇒ device powers on lit, regardless of last state.
    let mut s = OnOffState::new();
    s.start_up = StartUpOnOff::On;
    s.power_on();
    assert!(s.on);

    // StartUpOnOff::Off ⇒ powers on dark.
    let mut s = OnOffState::new();
    s.on = true;
    s.start_up = StartUpOnOff::Off;
    s.power_on();
    assert!(!s.on);

    // StartUpOnOff::Toggle ⇒ inverts the persisted state.
    let mut s = OnOffState::new();
    s.on = true;
    s.start_up = StartUpOnOff::Toggle;
    s.power_on();
    assert!(!s.on);

    // StartUpOnOff::Previous ⇒ keeps the persisted state.
    let mut s = OnOffState::new();
    s.on = true;
    s.start_up = StartUpOnOff::Previous;
    s.power_on();
    assert!(s.on);
}
