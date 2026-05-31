// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §3.2 (CSA public PDF) only; Z2M source NOT consulted.
//! Color Control cluster (0x0300) — ZCL §3.2.
//!
//! RED: references `cave_home_zigbee::color_control`, not yet implemented.
//! Wire vectors hand-computed from the ZCL §3.2.11 command tables.

use cave_home_zigbee::color_control::{
    attribute_id, command_id, ColorCommand, ColorControlState, ColorMode, ColorMoveMode,
    ColorStepMode, HueDirection, COLOR_CONTROL_CLUSTER_ID,
};

#[test]
fn cluster_and_command_ids_match_spec() {
    assert_eq!(COLOR_CONTROL_CLUSTER_ID, 0x0300);
    // §3.2.11 received-command table.
    assert_eq!(command_id::MOVE_TO_HUE, 0x00);
    assert_eq!(command_id::MOVE_HUE, 0x01);
    assert_eq!(command_id::STEP_HUE, 0x02);
    assert_eq!(command_id::MOVE_TO_SATURATION, 0x03);
    assert_eq!(command_id::MOVE_SATURATION, 0x04);
    assert_eq!(command_id::STEP_SATURATION, 0x05);
    assert_eq!(command_id::MOVE_TO_HUE_AND_SATURATION, 0x06);
    assert_eq!(command_id::MOVE_TO_COLOR, 0x07);
    assert_eq!(command_id::MOVE_TO_COLOR_TEMPERATURE, 0x0a);
}

#[test]
fn attribute_ids_match_spec() {
    // §3.2.2.2 attribute table.
    assert_eq!(attribute_id::CURRENT_HUE, 0x0000);
    assert_eq!(attribute_id::CURRENT_SATURATION, 0x0001);
    assert_eq!(attribute_id::CURRENT_X, 0x0003);
    assert_eq!(attribute_id::CURRENT_Y, 0x0004);
    assert_eq!(attribute_id::COLOR_MODE, 0x0008);
    assert_eq!(attribute_id::COLOR_TEMPERATURE_MIREDS, 0x0007);
    assert_eq!(attribute_id::COLOR_CAPABILITIES, 0x400a);
}

#[test]
fn color_mode_round_trips() {
    // §3.2.2.2.8 ColorMode enum8.
    for (v, e) in [
        (0x00u8, ColorMode::CurrentHueAndSaturation),
        (0x01, ColorMode::CurrentXy),
        (0x02, ColorMode::ColorTemperatureMireds),
    ] {
        assert_eq!(ColorMode::from_u8(v).unwrap(), e);
        assert_eq!(e.to_u8(), v);
    }
    assert!(ColorMode::from_u8(0x03).is_err());
}

#[test]
fn hue_direction_round_trips() {
    // §3.2.11.2 direction field.
    for (v, e) in [
        (0x00u8, HueDirection::ShortestDistance),
        (0x01, HueDirection::LongestDistance),
        (0x02, HueDirection::Up),
        (0x03, HueDirection::Down),
    ] {
        assert_eq!(HueDirection::from_u8(v).unwrap(), e);
        assert_eq!(e.to_u8(), v);
    }
    assert!(HueDirection::from_u8(0x04).is_err());
}

#[test]
fn move_mode_and_step_mode_round_trip() {
    // §3.2.11.3 move mode: 0x00 stop, 0x01 up, 0x03 down (0x02 reserved).
    assert_eq!(ColorMoveMode::from_u8(0x00).unwrap(), ColorMoveMode::Stop);
    assert_eq!(ColorMoveMode::from_u8(0x01).unwrap(), ColorMoveMode::Up);
    assert_eq!(ColorMoveMode::from_u8(0x03).unwrap(), ColorMoveMode::Down);
    assert!(ColorMoveMode::from_u8(0x02).is_err());
    // §3.2.11.4 step mode: 0x01 up, 0x03 down.
    assert_eq!(ColorStepMode::from_u8(0x01).unwrap(), ColorStepMode::Up);
    assert_eq!(ColorStepMode::from_u8(0x03).unwrap(), ColorStepMode::Down);
    assert!(ColorStepMode::from_u8(0x00).is_err());
    assert!(ColorStepMode::from_u8(0x02).is_err());
}

#[test]
fn parse_move_to_hue() {
    // §3.2.11.1: hue (u8) + direction (u8) + transition_time (u16 LE).
    let cmd = ColorCommand::parse(command_id::MOVE_TO_HUE, &[0x40, 0x00, 0x0a, 0x00]).unwrap();
    assert_eq!(
        cmd,
        ColorCommand::MoveToHue {
            hue: 0x40,
            direction: HueDirection::ShortestDistance,
            transition_time: 10,
        }
    );
}

#[test]
fn parse_step_hue() {
    // §3.2.11.5: step_mode (u8) + step_size (u8) + transition_time (u8).
    let cmd = ColorCommand::parse(command_id::STEP_HUE, &[0x01, 0x10, 0x05]).unwrap();
    assert_eq!(
        cmd,
        ColorCommand::StepHue {
            mode: ColorStepMode::Up,
            step_size: 0x10,
            transition_time: 0x05,
        }
    );
}

#[test]
fn parse_move_to_hue_and_saturation() {
    // §3.2.11.7: hue (u8) + saturation (u8) + transition_time (u16 LE).
    let cmd =
        ColorCommand::parse(command_id::MOVE_TO_HUE_AND_SATURATION, &[0x20, 0xfe, 0x14, 0x00]).unwrap();
    assert_eq!(
        cmd,
        ColorCommand::MoveToHueAndSaturation {
            hue: 0x20,
            saturation: 0xfe,
            transition_time: 20,
        }
    );
}

#[test]
fn parse_move_to_color() {
    // §3.2.11.8: color_x (u16 LE) + color_y (u16 LE) + transition_time (u16 LE).
    // x = 0x4ccc, y = 0x3333, t = 0x000a.
    let cmd =
        ColorCommand::parse(command_id::MOVE_TO_COLOR, &[0xcc, 0x4c, 0x33, 0x33, 0x0a, 0x00]).unwrap();
    assert_eq!(
        cmd,
        ColorCommand::MoveToColor {
            color_x: 0x4ccc,
            color_y: 0x3333,
            transition_time: 10,
        }
    );
}

#[test]
fn parse_move_to_color_temperature() {
    // §3.2.11.11: color_temp_mireds (u16 LE) + transition_time (u16 LE).
    // 0x00fa = 250 mireds (4000 K), t = 0x0005.
    let cmd =
        ColorCommand::parse(command_id::MOVE_TO_COLOR_TEMPERATURE, &[0xfa, 0x00, 0x05, 0x00]).unwrap();
    assert_eq!(
        cmd,
        ColorCommand::MoveToColorTemperature {
            color_temp_mireds: 250,
            transition_time: 5,
        }
    );
}

#[test]
fn parse_rejects_truncated_and_unknown() {
    assert!(ColorCommand::parse(command_id::MOVE_TO_HUE, &[0x40, 0x00]).is_err());
    assert!(ColorCommand::parse(command_id::MOVE_TO_COLOR, &[0xcc, 0x4c, 0x33]).is_err());
    assert!(ColorCommand::parse(0xff, &[]).is_err());
}

#[test]
fn command_round_trips() {
    for cmd in [
        ColorCommand::MoveToHue {
            hue: 100,
            direction: HueDirection::Up,
            transition_time: 5,
        },
        ColorCommand::MoveHue {
            mode: ColorMoveMode::Down,
            rate: 20,
        },
        ColorCommand::StepHue {
            mode: ColorStepMode::Down,
            step_size: 30,
            transition_time: 2,
        },
        ColorCommand::MoveToSaturation {
            saturation: 200,
            transition_time: 8,
        },
        ColorCommand::MoveSaturation {
            mode: ColorMoveMode::Up,
            rate: 15,
        },
        ColorCommand::StepSaturation {
            mode: ColorStepMode::Up,
            step_size: 40,
            transition_time: 3,
        },
        ColorCommand::MoveToHueAndSaturation {
            hue: 50,
            saturation: 150,
            transition_time: 12,
        },
        ColorCommand::MoveToColor {
            color_x: 0x1234,
            color_y: 0x5678,
            transition_time: 9,
        },
        ColorCommand::MoveToColorTemperature {
            color_temp_mireds: 370,
            transition_time: 7,
        },
    ] {
        let id = cmd.command_id();
        let payload = cmd.encode_payload();
        assert_eq!(ColorCommand::parse(id, &payload).unwrap(), cmd);
    }
}

#[test]
fn state_defaults() {
    let s = ColorControlState::new();
    // §3.2.2.2.8: default ColorMode is CurrentXy.
    assert_eq!(s.color_mode, ColorMode::CurrentXy);
}

#[test]
fn apply_move_to_hue_and_saturation_sets_hue_sat_mode() {
    let mut s = ColorControlState::new();
    s.apply(&ColorCommand::MoveToHueAndSaturation {
        hue: 80,
        saturation: 200,
        transition_time: 0,
    });
    assert_eq!(s.current_hue, 80);
    assert_eq!(s.current_saturation, 200);
    assert_eq!(s.color_mode, ColorMode::CurrentHueAndSaturation);
}

#[test]
fn apply_move_to_color_sets_xy_mode() {
    let mut s = ColorControlState::new();
    s.apply(&ColorCommand::MoveToColor {
        color_x: 0x4000,
        color_y: 0x2000,
        transition_time: 0,
    });
    assert_eq!(s.current_x, 0x4000);
    assert_eq!(s.current_y, 0x2000);
    assert_eq!(s.color_mode, ColorMode::CurrentXy);
}

#[test]
fn apply_move_to_color_temperature_sets_temp_mode() {
    let mut s = ColorControlState::new();
    s.apply(&ColorCommand::MoveToColorTemperature {
        color_temp_mireds: 250,
        transition_time: 0,
    });
    assert_eq!(s.color_temperature_mireds, 250);
    assert_eq!(s.color_mode, ColorMode::ColorTemperatureMireds);
}

#[test]
fn apply_step_hue_wraps_circular() {
    // Hue is circular over 0..=254 (0xff reserved); stepping up past 254 wraps.
    let mut s = ColorControlState::new();
    s.current_hue = 250;
    s.apply(&ColorCommand::StepHue {
        mode: ColorStepMode::Up,
        step_size: 10,
        transition_time: 0,
    });
    assert_eq!(s.current_hue, 5); // (250 + 10) mod 255
    s.current_hue = 5;
    s.apply(&ColorCommand::StepHue {
        mode: ColorStepMode::Down,
        step_size: 10,
        transition_time: 0,
    });
    assert_eq!(s.current_hue, 250); // (5 - 10) mod 255
}

#[test]
fn apply_step_saturation_saturates() {
    let mut s = ColorControlState::new();
    s.current_saturation = 250;
    s.apply(&ColorCommand::StepSaturation {
        mode: ColorStepMode::Up,
        step_size: 20,
        transition_time: 0,
    });
    assert_eq!(s.current_saturation, 254); // clamped, not wrapped
    s.current_saturation = 10;
    s.apply(&ColorCommand::StepSaturation {
        mode: ColorStepMode::Down,
        step_size: 20,
        transition_time: 0,
    });
    assert_eq!(s.current_saturation, 0);
}
