// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/dpt/dpt_1.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/dpt/dpt_5.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/dpt/dpt_9.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/dpt/dpt_14.py
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! KNX Datapoint Types (DPTs) — encode/decode raw bus bytes ↔ semantic
//! values. We port the four DPTs that cover the vast majority of
//! residential KNX traffic (and that ADR-011 spec'd as the Phase-1 cover):
//!
//! * **DPT 1.001** `DPT_Switch` — 1-bit (`on` / `off`).
//! * **DPT 5.001** `DPT_Scaling` — 1-byte 0..100 % linearly mapped to 0..=255.
//! * **DPT 9.001** `DPT_Value_Temp` — 2-byte half-precision IEEE-style
//!   `(sign, 4-bit exponent, 11-bit signed mantissa) * 0.01 °C`.
//! * **DPT 14.x** `DPT_4ByteFloat` — IEEE-754 single precision big-endian.

use crate::error::{KnxError, Result};

/// DPT 1.001 — switch. Encoded as a single bit in the lower nibble of an
/// APCI byte (xknx's `DPTBinary`); we expose it here as a simple boolean
/// codec to keep the surface small.
pub mod dpt_1 {
    use super::{KnxError, Result};

    /// Encode `bool` into the 1-bit small payload value (0 or 1).
    #[must_use]
    pub const fn to_knx(value: bool) -> u8 {
        if value { 1 } else { 0 }
    }

    /// Decode the 1-bit small payload value back to `bool`.
    pub fn from_knx(byte: u8) -> Result<bool> {
        match byte & 0x3F {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(KnxError::Conversion(format!(
                "DPT 1.001: expected 0 or 1, got {other}"
            ))),
        }
    }
}

/// DPT 5.001 — 1-byte percentage scaling 0..=100 %.
///
/// Encoding: `knx = round((value - 0) / (100 - 0) * 255)`.
/// Decoding: `value = round((knx / 255) * 100)`.
pub mod dpt_5_001 {
    use super::{KnxError, Result};

    pub const VALUE_MIN: u16 = 0;
    pub const VALUE_MAX: u16 = 100;

    /// Encode a 0..=100 percent value into 1 byte.
    pub fn to_knx(value: f64) -> Result<u8> {
        if !(VALUE_MIN as f64..=VALUE_MAX as f64).contains(&value) {
            return Err(KnxError::Conversion(format!(
                "DPT 5.001: value {value} out of range (0..=100)"
            )));
        }
        let delta = f64::from(VALUE_MAX - VALUE_MIN);
        let knx = ((value - f64::from(VALUE_MIN)) / delta * 255.0).round();
        Ok(knx as u8)
    }

    /// Decode the 1-byte payload back to a 0..=100 percentage.
    #[must_use]
    pub fn from_knx(byte: u8) -> u16 {
        let delta = VALUE_MAX - VALUE_MIN;
        ((f64::from(byte) / 255.0) * f64::from(delta)).round() as u16 + VALUE_MIN
    }
}

/// DPT 9.001 — 2-byte half-precision-like float, used for temperatures.
///
/// Big-endian layout (xknx `dpt_9.py`):
/// ```text
///   bit 15      | bit 14..11    | bit 10..0
///   sign (1b)   | exponent (4b) | mantissa (11b, two's complement when sign=1)
/// ```
/// value = `mantissa * 2^exponent * 0.01`.
pub mod dpt_9 {
    use super::{KnxError, Result};

    pub const VALUE_MIN: f64 = -671_088.64;
    pub const VALUE_MAX: f64 = 670_760.96;

    /// Encode an `f64` (e.g. degrees Celsius) into 2 bytes.
    pub fn to_knx(value: f64) -> Result<[u8; 2]> {
        if !(VALUE_MIN..=VALUE_MAX).contains(&value) {
            return Err(KnxError::Conversion(format!(
                "DPT 9.001: value {value} out of range"
            )));
        }
        let mut knx = value * 100.0;
        if knx.round() == 0.0 {
            // ETS rounds near-zero values to 0x0000 (per xknx upstream).
            return Ok([0x00, 0x00]);
        }
        let mut exponent: u8 = 0;
        while !(-2048.0..=2047.0).contains(&knx) {
            exponent += 1;
            knx /= 2.0;
        }
        let mantissa = (knx.round() as i32) & 0x7FF;
        let mut msb: u8 = (exponent << 3) | ((mantissa >> 8) as u8 & 0x07);
        if knx < 0.0 {
            msb |= 0x80;
        }
        Ok([msb, (mantissa & 0xFF) as u8])
    }

    /// Decode 2 bytes back into an `f64` (e.g. degrees Celsius).
    pub fn from_knx(raw: [u8; 2]) -> Result<f64> {
        let data = (u16::from(raw[0]) << 8) | u16::from(raw[1]);
        let exponent = ((data >> 11) & 0x0F) as i32;
        let mut significand = (data & 0x7FF) as i32;
        let sign = data >> 15;
        if sign == 1 {
            significand -= 2048;
        }
        let value = (significand << exponent) as f64 / 100.0;
        if !(VALUE_MIN..=VALUE_MAX).contains(&value) {
            return Err(KnxError::Conversion(format!(
                "DPT 9.001: decoded value {value} out of range"
            )));
        }
        Ok(value)
    }
}

/// DPT 14.x — IEEE-754 single-precision float, big-endian.
pub mod dpt_14 {
    use super::Result;

    #[must_use]
    pub fn to_knx(value: f32) -> [u8; 4] {
        value.to_be_bytes()
    }

    pub fn from_knx(raw: [u8; 4]) -> Result<f32> {
        Ok(f32::from_be_bytes(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpt_1_roundtrip() {
        assert_eq!(dpt_1::from_knx(dpt_1::to_knx(true)).unwrap(), true);
        assert_eq!(dpt_1::from_knx(dpt_1::to_knx(false)).unwrap(), false);
    }

    #[test]
    fn dpt_5_001_endpoints_and_midpoint() {
        assert_eq!(dpt_5_001::to_knx(0.0).unwrap(), 0);
        assert_eq!(dpt_5_001::to_knx(100.0).unwrap(), 255);
        // 50 % maps to 128 per xknx's `round` rule (127.5 → 128).
        assert_eq!(dpt_5_001::to_knx(50.0).unwrap(), 128);
        assert_eq!(dpt_5_001::from_knx(0), 0);
        assert_eq!(dpt_5_001::from_knx(255), 100);
        assert_eq!(dpt_5_001::from_knx(128), 50);
    }

    #[test]
    fn dpt_5_001_out_of_range() {
        assert!(dpt_5_001::to_knx(-1.0).is_err());
        assert!(dpt_5_001::to_knx(101.0).is_err());
    }

    #[test]
    fn dpt_9_001_zero() {
        let bytes = dpt_9::to_knx(0.0).unwrap();
        assert_eq!(bytes, [0x00, 0x00]);
        assert_eq!(dpt_9::from_knx([0x00, 0x00]).unwrap(), 0.0);
    }

    #[test]
    fn dpt_9_001_room_temperature() {
        // 21.5 °C — common heating set-point.
        let v = 21.5;
        let bytes = dpt_9::to_knx(v).unwrap();
        let decoded = dpt_9::from_knx(bytes).unwrap();
        assert!((decoded - v).abs() < 0.05, "decoded={decoded} vs {v}");
    }

    #[test]
    fn dpt_9_001_negative() {
        // -10.0 °C — frost set-point.
        let v = -10.0;
        let bytes = dpt_9::to_knx(v).unwrap();
        let decoded = dpt_9::from_knx(bytes).unwrap();
        assert!((decoded - v).abs() < 0.05, "decoded={decoded} vs {v}");
    }

    #[test]
    fn dpt_9_001_out_of_range() {
        assert!(dpt_9::to_knx(1_000_000.0).is_err());
        assert!(dpt_9::to_knx(-1_000_000.0).is_err());
    }

    #[test]
    fn dpt_14_roundtrip() {
        let v: f32 = core::f32::consts::PI;
        let bytes = dpt_14::to_knx(v);
        let decoded = dpt_14::from_knx(bytes).unwrap();
        assert!((decoded - v).abs() < 1e-6);
    }

    #[test]
    fn dpt_14_negative() {
        let v: f32 = -42.5;
        let bytes = dpt_14::to_knx(v);
        assert_eq!(dpt_14::from_knx(bytes).unwrap(), v);
    }
}
