// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Integration tests (against the public API) for the NEW datapoint codecs:
//! DPT 10.001 (time-of-day) and DPT 11.001 (date). Both are 3-byte payloads.
//!
//! These tests target functions that DO NOT YET EXIST — they are the RED phase
//! of strict TDD. They follow the crate's existing per-DPT module convention
//! (`dptN` module exposing a value struct plus `encode`/`decode`):
//!
//!   dpt10::Time { day: u8, hour: u8, minute: u8, second: u8 }
//!   dpt10::encode(Time) -> Result<[u8; 3]>
//!   dpt10::decode(&[u8]) -> Result<Time>
//!
//!   dpt11::Date { day: u8, month: u8, year: u16 }
//!   dpt11::encode(Date) -> Result<[u8; 3]>
//!   dpt11::decode(&[u8]) -> Result<Date>
//!
//! `day` for Time is the day-of-week: 0 = no day, 1 = Mon .. 7 = Sun.

use cave_home_knx::dpt::{dpt10, dpt11};

// ---------------------------------------------------------------------------
// DPT 10.001 — Time-of-day.
//
// Layout (3 bytes):
//   byte0: bits 7..5 = day-of-week (0..=7), bits 4..0 = hour (0..=23)
//   byte1: bits 5..0 = minute (0..=59)   (bits 7..6 reserved = 0)
//   byte2: bits 5..0 = second (0..=59)   (bits 7..6 reserved = 0)
// ---------------------------------------------------------------------------

#[test]
fn dpt10_time_13_45_30_wednesday_encodes_and_round_trips() {
    // 13:45:30, Wednesday => day-of-week = 3.
    //
    // byte0 = (day << 5) | hour = (3 << 5) | 13
    //       = 0b011_00000 | 0b00001101
    //       = 0x60 | 0x0D
    //       = 0x6D                                   (96 + 13 = 109)
    // byte1 = minute = 45 = 0x2D                      (32 + 13 = 45)
    // byte2 = second = 30 = 0x1E                      (16 + 14 = 30)
    let t = dpt10::Time {
        day: 3,
        hour: 13,
        minute: 45,
        second: 30,
    };
    let bytes = dpt10::encode(t).unwrap();
    assert_eq!(bytes, [0x6D, 0x2D, 0x1E]);

    // Decode round-trip.
    let back = dpt10::decode(&bytes).unwrap();
    assert_eq!(back, t);
    assert_eq!(back.day, 3);
    assert_eq!(back.hour, 13);
    assert_eq!(back.minute, 45);
    assert_eq!(back.second, 30);
}

#[test]
fn dpt10_time_midnight_no_day_is_all_zero() {
    // 00:00:00 with no day-of-week (day = 0).
    //
    // byte0 = (0 << 5) | 0 = 0x00
    // byte1 = 0x00
    // byte2 = 0x00
    let t = dpt10::Time {
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
    };
    let bytes = dpt10::encode(t).unwrap();
    assert_eq!(bytes, [0x00, 0x00, 0x00]);
    assert_eq!(dpt10::decode(&bytes).unwrap(), t);
}

#[test]
fn dpt10_time_rejects_out_of_range_fields() {
    // hour 24 is out of range (valid 0..=23) — must error, never panic.
    assert!(dpt10::encode(dpt10::Time {
        day: 0,
        hour: 24,
        minute: 0,
        second: 0,
    })
    .is_err());

    // minute 60 is out of range (valid 0..=59).
    assert!(dpt10::encode(dpt10::Time {
        day: 0,
        hour: 0,
        minute: 60,
        second: 0,
    })
    .is_err());

    // second 60 is out of range (valid 0..=59).
    assert!(dpt10::encode(dpt10::Time {
        day: 0,
        hour: 0,
        minute: 0,
        second: 60,
    })
    .is_err());

    // day 8 is out of range (valid 0..=7).
    assert!(dpt10::encode(dpt10::Time {
        day: 8,
        hour: 0,
        minute: 0,
        second: 0,
    })
    .is_err());
}

#[test]
fn dpt10_time_rejects_wrong_payload_length() {
    // 3-byte payload required; anything else must error.
    assert!(dpt10::decode(&[0x00, 0x00]).is_err());
    assert!(dpt10::decode(&[0x00, 0x00, 0x00, 0x00]).is_err());
}

// ---------------------------------------------------------------------------
// DPT 11.001 — Date.
//
// Layout (3 bytes):
//   byte0: bits 4..0 = day (1..=31)
//   byte1: bits 3..0 = month (1..=12)
//   byte2: bits 6..0 = year, where 0..=89 => 2000..=2089 and 90..=99 => 1990..=1999
// ---------------------------------------------------------------------------

#[test]
fn dpt11_date_2024_03_15_encodes_and_round_trips() {
    // 2024-03-15.
    //
    // byte0 = day   = 15 = 0x0F
    // byte1 = month = 3  = 0x03
    // byte2 = year code: 2024 is in 2000..=2089, so code = 2024 - 2000 = 24 = 0x18
    let d = dpt11::Date {
        day: 15,
        month: 3,
        year: 2024,
    };
    let bytes = dpt11::encode(d).unwrap();
    assert_eq!(bytes, [0x0F, 0x03, 0x18]);

    // Decode round-trip => year reconstructs as 2024.
    let back = dpt11::decode(&bytes).unwrap();
    assert_eq!(back, d);
    assert_eq!(back.day, 15);
    assert_eq!(back.month, 3);
    assert_eq!(back.year, 2024);
}

#[test]
fn dpt11_date_year_code_mapping() {
    // byte2 = 95 is in 90..=99 => maps to 1990 + (95 - 90) = 1995.
    // day = 1, month = 1 => byte0 = 0x01, byte1 = 0x01.
    let d95 = dpt11::decode(&[0x01, 0x01, 95]).unwrap();
    assert_eq!(d95.year, 1995);

    // byte2 = 0 is in 0..=89 => maps to 2000 + 0 = 2000.
    let d00 = dpt11::decode(&[0x01, 0x01, 0]).unwrap();
    assert_eq!(d00.year, 2000);
}

#[test]
fn dpt11_date_rejects_out_of_range_fields() {
    // month 13 is out of range (valid 1..=12) — must error, never panic.
    assert!(dpt11::encode(dpt11::Date {
        day: 1,
        month: 13,
        year: 2024,
    })
    .is_err());

    // day 0 is out of range (valid 1..=31).
    assert!(dpt11::encode(dpt11::Date {
        day: 0,
        month: 1,
        year: 2024,
    })
    .is_err());

    // day 32 is out of range (valid 1..=31).
    assert!(dpt11::encode(dpt11::Date {
        day: 32,
        month: 1,
        year: 2024,
    })
    .is_err());
}

#[test]
fn dpt11_date_rejects_wrong_payload_length() {
    // 3-byte payload required; anything else must error.
    assert!(dpt11::decode(&[0x01, 0x01]).is_err());
    assert!(dpt11::decode(&[0x01, 0x01, 0x00, 0x00]).is_err());
}
