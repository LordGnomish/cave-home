// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! RED-phase test for the NEW SunSpec Model 203 meter decode.
//!
//! Model 203 is the public-standard three-phase (wye/delta) meter model. Like
//! the inverter integer models, every measurement is a raw integer paired with
//! a `sunssf` power-of-ten scale factor; energy counters are `acc32`. This test
//! drives a not-yet-existing `MeterReading` + `decode_meter(...)` that mirror
//! the crate's existing `InverterReading` + `decode_inverter(...)` pattern
//! (`crates/cave-home-solar-sunspec/src/inverter.rs`, `src/lib.rs`).
//!
//! # Model 203 payload layout — register offsets used here
//!
//! Offsets are 0-based within the model's *payload* (i.e. after the 2-word
//! `id + length` header that `discover`/`walk_chain` already strips). Taken
//! from the public SunSpec Information Model Specification, meter model 203
//! ("Wye-Connect Three Phase (Abcn) Meter"):
//!
//! ```text
//!    0  A         Total AC current               int16   (A_SF @ 4)
//!    4  A_SF      AC current scale factor         sunssf
//!   18  W         Total real power                int16   (W_SF @ 22)
//!   22  W_SF      Real power scale factor         sunssf
//!  152  TotWhExp  Total real energy exported      acc32   (2 regs, hi word first)
//!  154  TotWhImp  Total real energy imported      acc32   (2 regs, hi word first)
//!  158  TotWh_SF  Real energy scale factor        sunssf
//!  164  Evt       Meter event flags               bitfield32 (2 regs, hi first)
//! ```
//!
//! For this feature we decode: total real power `W` (scaled), total imported
//! real energy `TotWhImp` (Wh, `acc32`), total exported real energy
//! `TotWhExp`, and the presence of any meter event flag.

use cave_home_solar_sunspec::{DecodeError, MeterReading, decode_meter};

/// Build a Model 203 payload long enough to reach the energy + event block.
/// `Evt` ends at offset 165, so 166 registers is the minimum useful payload.
fn build_meter_203() -> Vec<u16> {
    let mut p = vec![0u16; 166];

    // --- Total AC current: A raw = 250, A_SF = -1  =>  25.0 A ---
    p[0] = 250; // A raw (int16)
    p[4] = (-1i16) as u16; // A_SF = -1

    // --- Total real power: W raw = 1500, W_SF = 0  =>  1500 W ---
    p[18] = 1500u16; // W raw (int16)
    p[22] = 0; // W_SF = 0  => 1500 * 10^0 = 1500 W

    // --- Total real energy exported: TotWhExp acc32 ---
    // 0x0000_2710 = 10_000  =>  with TotWh_SF = 0  =>  10_000 Wh
    p[152] = 0x0000; // TotWhExp hi word
    p[153] = 0x2710; // TotWhExp lo word  => 0x00002710 = 10_000

    // --- Total real energy imported: TotWhImp acc32 ---
    // high = 0x0001, low = 0x86A0  =>  0x000186A0 = 100_000 Wh
    p[154] = 0x0001; // TotWhImp hi word
    p[155] = 0x86A0; // TotWhImp lo word  => 0x000186A0 = 100_000

    // --- Energy scale factor: TotWh_SF = 0  => energy as-is in Wh ---
    p[158] = 0; // TotWh_SF = 0

    // --- Meter event flags: 0 => no events present ---
    p[164] = 0x0000; // Evt hi
    p[165] = 0x0000; // Evt lo

    p
}

#[test]
fn decode_model_203_total_real_power_scaled() {
    let p = build_meter_203();
    let r: MeterReading = decode_meter(203, &p).expect("model 203 decodes");

    // W raw 1500 * 10^(W_SF=0) = 1500 W
    assert!((r.total_real_power_w.unwrap() - 1500.0).abs() < 1e-6);
}

#[test]
fn decode_model_203_power_scale_factor_applied() {
    // W_SF = 1  =>  1500 * 10^1 = 15_000 W
    let mut p = build_meter_203();
    p[18] = 1500u16; // W raw
    p[22] = 1; // W_SF = 1
    let r = decode_meter(203, &p).expect("decodes");
    assert!((r.total_real_power_w.unwrap() - 15_000.0).abs() < 1e-6);
}

#[test]
fn decode_model_203_imported_energy_acc32() {
    let p = build_meter_203();
    let r = decode_meter(203, &p).expect("decodes");

    // TotWhImp: hi=0x0001, lo=0x86A0 => 0x000186A0 = 100_000; TotWh_SF=0 => 100_000 Wh
    assert!((r.total_energy_imported_wh.unwrap() - 100_000.0).abs() < 1e-6);
}

#[test]
fn decode_model_203_exported_energy_acc32() {
    let p = build_meter_203();
    let r = decode_meter(203, &p).expect("decodes");

    // TotWhExp: 0x00002710 = 10_000; TotWh_SF=0 => 10_000 Wh
    assert!((r.total_energy_exported_wh.unwrap() - 10_000.0).abs() < 1e-6);
}

#[test]
fn decode_model_203_no_events_present() {
    let p = build_meter_203();
    let r = decode_meter(203, &p).expect("decodes");
    // Evt = 0 => no meter events flagged.
    assert!(!r.has_event);
}

#[test]
fn decode_model_203_event_flag_present() {
    let mut p = build_meter_203();
    p[164] = 0x0000;
    p[165] = 0x0004; // some non-zero event bit set
    let r = decode_meter(203, &p).expect("decodes");
    assert!(r.has_event);
}

#[test]
fn never_accumulated_import_energy_is_none() {
    // acc32 == 0 is the SunSpec "not accumulated" sentinel => None, not 0.
    let mut p = build_meter_203();
    p[154] = 0;
    p[155] = 0;
    let r = decode_meter(203, &p).expect("decodes");
    assert!(r.total_energy_imported_wh.is_none());
}

#[test]
fn unimplemented_power_is_none() {
    // int16 sentinel 0x8000 => the meter does not implement total real power.
    let mut p = build_meter_203();
    p[18] = 0x8000; // INT16 not-implemented sentinel
    let r = decode_meter(203, &p).expect("decodes");
    assert!(r.total_real_power_w.is_none());
}

#[test]
fn truncated_payload_is_error_not_panic() {
    // Too short to reach the energy/event block => OutOfBounds, never a panic.
    let p = vec![0u16; 30];
    assert!(matches!(
        decode_meter(203, &p),
        Err(DecodeError::OutOfBounds { .. })
    ));
}

#[test]
fn non_meter_model_id_rejected() {
    let p = build_meter_203();
    assert!(matches!(
        decode_meter(103, &p),
        Err(DecodeError::UnsupportedModel { model_id: 103 })
    ));
}

#[test]
fn meter_reading_decode_integer_associated_fn() {
    // Mirror InverterReading::decode_integer — the associated constructor the
    // free `decode_meter` dispatches to.
    let p = build_meter_203();
    let r = MeterReading::decode_integer(203, &p).expect("decodes");
    assert!((r.total_real_power_w.unwrap() - 1500.0).abs() < 1e-6);
}
