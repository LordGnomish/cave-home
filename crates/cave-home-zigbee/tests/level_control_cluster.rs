// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §3.10 (CSA public PDF) only; Z2M source NOT consulted.
//! Level Control cluster (0x0008) — ZCL §3.10.
//!
//! RED: references `cave_home_zigbee::level_control`, not yet implemented.
//! Wire vectors hand-computed from the ZCL §3.10.2.4 command tables.

use cave_home_zigbee::level_control::{
    attribute_id, command_id, LevelCommand, LevelControlState, MoveMode, StartUpCurrentLevel,
    StepMode, LEVEL_CONTROL_CLUSTER_ID,
};

#[test]
fn cluster_and_command_ids_match_spec() {
    assert_eq!(LEVEL_CONTROL_CLUSTER_ID, 0x0008);
    // ZCL §3.10.2.4 received-command table.
    assert_eq!(command_id::MOVE_TO_LEVEL, 0x00);
    assert_eq!(command_id::MOVE, 0x01);
    assert_eq!(command_id::STEP, 0x02);
    assert_eq!(command_id::STOP, 0x03);
    assert_eq!(command_id::MOVE_TO_LEVEL_WITH_ON_OFF, 0x04);
    assert_eq!(command_id::MOVE_WITH_ON_OFF, 0x05);
    assert_eq!(command_id::STEP_WITH_ON_OFF, 0x06);
    assert_eq!(command_id::STOP_WITH_ON_OFF, 0x07);
}

#[test]
fn attribute_ids_match_spec() {
    // ZCL §3.10.2.3 attribute table.
    assert_eq!(attribute_id::CURRENT_LEVEL, 0x0000);
    assert_eq!(attribute_id::REMAINING_TIME, 0x0001);
    assert_eq!(attribute_id::OPTIONS, 0x000f);
    assert_eq!(attribute_id::ON_LEVEL, 0x0011);
    assert_eq!(attribute_id::START_UP_CURRENT_LEVEL, 0x4000);
}

#[test]
fn move_mode_round_trips() {
    // §3.10.2.4.2: 0x00 = up, 0x01 = down.
    assert_eq!(MoveMode::from_u8(0x00).unwrap(), MoveMode::Up);
    assert_eq!(MoveMode::from_u8(0x01).unwrap(), MoveMode::Down);
    assert_eq!(MoveMode::Up.to_u8(), 0x00);
    assert_eq!(MoveMode::Down.to_u8(), 0x01);
    assert!(MoveMode::from_u8(0x02).is_err());
}

#[test]
fn step_mode_round_trips() {
    assert_eq!(StepMode::from_u8(0x00).unwrap(), StepMode::Up);
    assert_eq!(StepMode::from_u8(0x01).unwrap(), StepMode::Down);
    assert!(StepMode::from_u8(0x02).is_err());
}

#[test]
fn parse_move_to_level() {
    // §3.10.2.4.1: level (u8) + transition_time (u16 LE).
    // level = 0x80 = 128, transition = 0x000a = 10 (1.0 s).
    let cmd = LevelCommand::parse(command_id::MOVE_TO_LEVEL, &[0x80, 0x0a, 0x00]).unwrap();
    assert_eq!(
        cmd,
        LevelCommand::MoveToLevel {
            level: 128,
            transition_time: 10,
            with_on_off: false,
        }
    );
    // The with-on/off variant carries the same payload but a different id.
    let cmd2 = LevelCommand::parse(command_id::MOVE_TO_LEVEL_WITH_ON_OFF, &[0x80, 0x0a, 0x00]).unwrap();
    assert_eq!(
        cmd2,
        LevelCommand::MoveToLevel {
            level: 128,
            transition_time: 10,
            with_on_off: true,
        }
    );
}

#[test]
fn parse_move() {
    // §3.10.2.4.2: move_mode (u8) + rate (u8).
    let cmd = LevelCommand::parse(command_id::MOVE, &[0x00, 0x32]).unwrap();
    assert_eq!(
        cmd,
        LevelCommand::Move {
            mode: MoveMode::Up,
            rate: 0x32,
            with_on_off: false,
        }
    );
}

#[test]
fn parse_step() {
    // §3.10.2.4.3: step_mode (u8) + step_size (u8) + transition_time (u16 LE).
    let cmd = LevelCommand::parse(command_id::STEP, &[0x01, 0x10, 0x05, 0x00]).unwrap();
    assert_eq!(
        cmd,
        LevelCommand::Step {
            mode: StepMode::Down,
            step_size: 0x10,
            transition_time: 5,
            with_on_off: false,
        }
    );
}

#[test]
fn parse_stop() {
    assert_eq!(
        LevelCommand::parse(command_id::STOP, &[]).unwrap(),
        LevelCommand::Stop { with_on_off: false }
    );
    assert_eq!(
        LevelCommand::parse(command_id::STOP_WITH_ON_OFF, &[]).unwrap(),
        LevelCommand::Stop { with_on_off: true }
    );
}

#[test]
fn parse_rejects_truncated() {
    assert!(LevelCommand::parse(command_id::MOVE_TO_LEVEL, &[0x80]).is_err());
    assert!(LevelCommand::parse(command_id::MOVE, &[0x00]).is_err());
    assert!(LevelCommand::parse(command_id::STEP, &[0x00, 0x10]).is_err());
}

#[test]
fn parse_rejects_unknown_command() {
    assert!(LevelCommand::parse(0x42, &[]).is_err());
}

#[test]
fn command_round_trips() {
    for cmd in [
        LevelCommand::MoveToLevel {
            level: 200,
            transition_time: 20,
            with_on_off: false,
        },
        LevelCommand::MoveToLevel {
            level: 0,
            transition_time: 0,
            with_on_off: true,
        },
        LevelCommand::Move {
            mode: MoveMode::Down,
            rate: 50,
            with_on_off: true,
        },
        LevelCommand::Step {
            mode: StepMode::Up,
            step_size: 25,
            transition_time: 3,
            with_on_off: false,
        },
        LevelCommand::Stop { with_on_off: true },
    ] {
        let id = cmd.command_id();
        let payload = cmd.encode_payload();
        assert_eq!(LevelCommand::parse(id, &payload).unwrap(), cmd);
    }
}

#[test]
fn state_defaults() {
    let s = LevelControlState::new();
    assert_eq!(s.current_level, 254); // §3.10.2.3.1 default 0xfe
    assert_eq!(s.start_up_current_level, StartUpCurrentLevel::Previous);
}

#[test]
fn apply_move_to_level_clamps() {
    let mut s = LevelControlState::new();
    s.apply(&LevelCommand::MoveToLevel {
        level: 100,
        transition_time: 0,
        with_on_off: false,
    });
    assert_eq!(s.current_level, 100);
    // 0xff is reserved → clamp to max valid level 0xfe.
    s.apply(&LevelCommand::MoveToLevel {
        level: 0xff,
        transition_time: 0,
        with_on_off: false,
    });
    assert_eq!(s.current_level, 254);
}

#[test]
fn apply_move_drives_to_boundary() {
    let mut s = LevelControlState::new();
    s.current_level = 100;
    s.apply(&LevelCommand::Move {
        mode: MoveMode::Up,
        rate: 50,
        with_on_off: false,
    });
    assert_eq!(s.current_level, 254);
    s.apply(&LevelCommand::Move {
        mode: MoveMode::Down,
        rate: 50,
        with_on_off: false,
    });
    assert_eq!(s.current_level, 0);
}

#[test]
fn apply_step_saturates() {
    let mut s = LevelControlState::new();
    s.current_level = 250;
    s.apply(&LevelCommand::Step {
        mode: StepMode::Up,
        step_size: 25,
        transition_time: 0,
        with_on_off: false,
    });
    assert_eq!(s.current_level, 254); // saturated, not wrapped
    s.current_level = 10;
    s.apply(&LevelCommand::Step {
        mode: StepMode::Down,
        step_size: 25,
        transition_time: 0,
        with_on_off: false,
    });
    assert_eq!(s.current_level, 0);
}

#[test]
fn apply_stop_is_noop_on_level() {
    let mut s = LevelControlState::new();
    s.current_level = 123;
    s.apply(&LevelCommand::Stop { with_on_off: false });
    assert_eq!(s.current_level, 123);
}

#[test]
fn move_to_level_with_on_off_reports_on_off_coupling() {
    // §3.10.2.4.4: the *WithOnOff commands also drive the OnOff attribute.
    let on = LevelCommand::MoveToLevel {
        level: 100,
        transition_time: 0,
        with_on_off: true,
    };
    assert_eq!(on.couples_on_off(), Some(true)); // level > 0 ⇒ turn on
    let off = LevelCommand::MoveToLevel {
        level: 0,
        transition_time: 0,
        with_on_off: true,
    };
    assert_eq!(off.couples_on_off(), Some(false)); // level 0 ⇒ turn off
    // Non-on/off variant never couples.
    let plain = LevelCommand::MoveToLevel {
        level: 0,
        transition_time: 0,
        with_on_off: false,
    };
    assert_eq!(plain.couples_on_off(), None);
}

#[test]
fn start_up_current_level_round_trips() {
    // §3.10.2.3.13: 0x00 = minimum, 0xff = previous, else = that level.
    assert_eq!(
        StartUpCurrentLevel::from_u8(0x00),
        StartUpCurrentLevel::Minimum
    );
    assert_eq!(
        StartUpCurrentLevel::from_u8(0xff),
        StartUpCurrentLevel::Previous
    );
    assert_eq!(
        StartUpCurrentLevel::from_u8(0x80),
        StartUpCurrentLevel::Level(0x80)
    );
    assert_eq!(StartUpCurrentLevel::Minimum.to_u8(), 0x00);
    assert_eq!(StartUpCurrentLevel::Previous.to_u8(), 0xff);
    assert_eq!(StartUpCurrentLevel::Level(0x80).to_u8(), 0x80);
}

#[test]
fn power_on_applies_start_up_level() {
    let mut s = LevelControlState::new();
    s.current_level = 42;
    s.start_up_current_level = StartUpCurrentLevel::Previous;
    s.power_on();
    assert_eq!(s.current_level, 42);

    s.start_up_current_level = StartUpCurrentLevel::Minimum;
    s.power_on();
    assert_eq!(s.current_level, 1);

    s.start_up_current_level = StartUpCurrentLevel::Level(150);
    s.power_on();
    assert_eq!(s.current_level, 150);
}
