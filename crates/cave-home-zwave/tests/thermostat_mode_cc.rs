// SPDX-License-Identifier: Apache-2.0
//! RED-phase integration test for the **Thermostat Mode Command Class (0x40)**.
//!
//! This drives a NEW feature that does not yet exist in `cave-home-zwave`:
//! decode/encode support for THERMOSTAT_MODE (CC id `0x40`). It is written
//! against the crate's *existing* public conventions so that Phase B can
//! implement exactly the shapes asserted here.
//!
//! ## Public API this test expects Phase B to add
//!
//! - A new `CommandClass::ThermostatMode` variant mapping to/from id `0x40`
//!   (i.e. `CommandClass::from_u8(0x40) == Some(CommandClass::ThermostatMode)`
//!   and `CommandClass::ThermostatMode.to_u8() == 0x40`).
//! - A new `ThermostatMode` enum (re-exported from the crate root, like
//!   `LevelChange`) with the spec's named modes:
//!     * `Off`                 = 0x00
//!     * `Heat`                = 0x01
//!     * `Cool`                = 0x02
//!     * `Auto`                = 0x03
//!     * `EnergySaveHeat`      = 0x0B
//!     * `EnergySaveCool`      = 0x0C
//!     * `ManufacturerSpecific`= 0x1F
//!   with `ThermostatMode::from_u8(u8) -> Option<Self>` and
//!   `ThermostatMode::to_u8(self) -> u8`, mirroring the `CommandClass` pattern.
//! - Three new `Command` variants, tuple-style like `BasicSet`/`BasicReport`
//!   because the mode is a single scalar field:
//!     * `Command::ThermostatModeSet(ThermostatMode)`
//!     * `Command::ThermostatModeGet`
//!     * `Command::ThermostatModeReport(ThermostatMode)`
//!
//! ## Wire layout (public Z-Wave CC spec, SDS13781 family)
//!
//! Command Class id `0x40` (THERMOSTAT_MODE). Within the class:
//!   * Set    cmd `0x01`: `[0x40, 0x01, mode]`  (mode = low 5 bits)
//!   * Get    cmd `0x02`: `[0x40, 0x02]`        (no body)
//!   * Report cmd `0x03`: `[0x40, 0x03, mode]`
//!
//! These reuse the crate's shared `SET=0x01 / GET=0x02 / REPORT=0x03` command
//! ids, exactly like Basic / Binary Switch / Multilevel Switch.
//!
//! ## Unknown-mode convention (documented choice)
//!
//! The crate rejects *unmodelled discrete values* with
//! `ZwaveError::OutOfRange { field, value }` (see Battery rejecting 101..=254
//! and Multilevel Switch rejecting the reserved 100..=254 level range). A
//! thermostat mode byte of `0x1E` is unassigned by the spec — it is neither one
//! of the named modes nor the `0x1F` ManufacturerSpecific sentinel — so this
//! test asserts the same `OutOfRange` rejection rather than fabricating a mode.
//! `0x1F` IS modelled (ManufacturerSpecific) and must decode successfully.

use cave_home_zwave::{Command, CommandClass, ThermostatMode, ZwaveError};

/// Round-trip helper: encode then decode must return the original command.
fn roundtrip(cmd: Command) {
    let bytes = cmd.encode();
    let back = Command::decode(&bytes).expect("re-decodes");
    assert_eq!(back, cmd, "round-trip mismatch for {cmd:?} via {bytes:02x?}");
}

#[test]
fn command_class_id_maps_to_thermostat_mode() {
    // 0x40 is THERMOSTAT_MODE.
    assert_eq!(CommandClass::from_u8(0x40), Some(CommandClass::ThermostatMode));
    assert_eq!(CommandClass::ThermostatMode.to_u8(), 0x40);
}

#[test]
fn mode_enum_byte_mapping() {
    // Each named mode maps to its spec byte and back.
    for (mode, byte) in [
        (ThermostatMode::Off, 0x00u8),
        (ThermostatMode::Heat, 0x01),
        (ThermostatMode::Cool, 0x02),
        (ThermostatMode::Auto, 0x03),
        (ThermostatMode::EnergySaveHeat, 0x0B),
        (ThermostatMode::EnergySaveCool, 0x0C),
        (ThermostatMode::ManufacturerSpecific, 0x1F),
    ] {
        assert_eq!(mode.to_u8(), byte);
        assert_eq!(ThermostatMode::from_u8(byte), Some(mode));
    }
    // 0x1E is unassigned — not a known mode.
    assert_eq!(ThermostatMode::from_u8(0x1E), None);
}

#[test]
fn decode_report_heat() {
    // [0x40, 0x03, 0x01] : CC=THERMOSTAT_MODE, cmd=REPORT, mode byte 0x01 = Heat.
    assert_eq!(
        Command::decode(&[0x40, 0x03, 0x01]).expect("valid report"),
        Command::ThermostatModeReport(ThermostatMode::Heat)
    );
}

#[test]
fn decode_report_cool() {
    // mode byte 0x02 = Cool.
    assert_eq!(
        Command::decode(&[0x40, 0x03, 0x02]).expect("valid report"),
        Command::ThermostatModeReport(ThermostatMode::Cool)
    );
}

#[test]
fn decode_report_off() {
    // mode byte 0x00 = Off.
    assert_eq!(
        Command::decode(&[0x40, 0x03, 0x00]).expect("valid report"),
        Command::ThermostatModeReport(ThermostatMode::Off)
    );
}

#[test]
fn decode_report_auto() {
    // mode byte 0x03 = Auto.
    assert_eq!(
        Command::decode(&[0x40, 0x03, 0x03]).expect("valid report"),
        Command::ThermostatModeReport(ThermostatMode::Auto)
    );
}

#[test]
fn encode_set_heat_known_bytes() {
    // Set(Heat): CC 0x40, cmd SET 0x01, mode 0x01 => [0x40, 0x01, 0x01].
    assert_eq!(
        Command::ThermostatModeSet(ThermostatMode::Heat).encode(),
        vec![0x40, 0x01, 0x01]
    );
}

#[test]
fn encode_get_known_bytes() {
    // Get carries no body: CC 0x40, cmd GET 0x02 => [0x40, 0x02].
    assert_eq!(Command::ThermostatModeGet.encode(), vec![0x40, 0x02]);
}

#[test]
fn set_and_report_roundtrip() {
    // Set round-trips: encode([0x40,0x01,mode]) decodes back to the same Set.
    roundtrip(Command::ThermostatModeSet(ThermostatMode::Heat));
    roundtrip(Command::ThermostatModeSet(ThermostatMode::Cool));
    roundtrip(Command::ThermostatModeSet(ThermostatMode::Off));
    roundtrip(Command::ThermostatModeSet(ThermostatMode::Auto));
    roundtrip(Command::ThermostatModeSet(ThermostatMode::EnergySaveHeat));
    roundtrip(Command::ThermostatModeSet(ThermostatMode::EnergySaveCool));
    roundtrip(Command::ThermostatModeSet(ThermostatMode::ManufacturerSpecific));

    // Report round-trips likewise.
    roundtrip(Command::ThermostatModeReport(ThermostatMode::Heat));
    roundtrip(Command::ThermostatModeReport(ThermostatMode::Auto));

    // Get round-trips (no body).
    roundtrip(Command::ThermostatModeGet);
}

#[test]
fn decode_truncated_report_missing_mode_is_error_not_panic() {
    // [0x40, 0x03] has the CC + REPORT cmd but no mode byte: must be Truncated,
    // and must NOT panic.
    assert!(matches!(
        Command::decode(&[0x40, 0x03]),
        Err(ZwaveError::Truncated { .. })
    ));
}

#[test]
fn decode_unknown_mode_is_out_of_range() {
    // 0x1E is an unassigned mode byte (not a named mode, not the 0x1F
    // ManufacturerSpecific sentinel). Per the crate's convention for unmodelled
    // discrete values, this is rejected with OutOfRange — see module docs.
    assert!(matches!(
        Command::decode(&[0x40, 0x03, 0x1E]),
        Err(ZwaveError::OutOfRange { .. })
    ));
}
