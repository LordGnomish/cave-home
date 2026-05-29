// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! KNX Datapoint Types (DPTs) — the codec that turns raw bus bytes into
//! semantic values and back.
//!
//! Every codec here is **pure byte logic**, implemented from the public KNX
//! datapoint-type tables (the encodings xknx — MIT — also implements; behavior
//! referenced, code first-party). Each function is total: it returns
//! [`Result`] and never panics, rejecting out-of-range values and wrong payload
//! lengths.
//!
//! DPTs covered (the residential core):
//!
//! | Main | Width   | Meaning                                            |
//! |------|---------|----------------------------------------------------|
//! | 1.x  | 1 bit   | boolean — switch / up-down / open-close            |
//! | 2.x  | 2 bit   | 1-bit value with a priority/control flag           |
//! | 3.x  | 4 bit   | dimming / blind control — direction + step code    |
//! | 5.x  | 1 byte  | unsigned — scaling 0..=100 %, angle 0..=360°, raw  |
//! | 6.x  | 1 byte  | signed (two's complement)                          |
//! | 7.x  | 2 byte  | unsigned 16-bit                                     |
//! | 8.x  | 2 byte  | signed 16-bit                                      |
//! | 9.x  | 2 byte  | the KNX 16-bit float (sign, 4-bit exp, 11-bit mant)|
//! | 12.x | 4 byte  | unsigned 32-bit                                     |
//! | 13.x | 4 byte  | signed 32-bit                                      |
//! | 14.x | 4 byte  | IEEE-754 single-precision float                    |
//! | 16.x | 14 byte | string (latin-1 / ASCII), null-padded              |

use crate::error::{KnxError, Result};

/// Helper: build a conversion error.
fn conv(msg: impl Into<String>) -> KnxError {
    KnxError::Conversion(msg.into())
}

/// Require an exact payload length, or fail.
fn expect_len(bytes: &[u8], want: usize, what: &str) -> Result<()> {
    if bytes.len() != want {
        return Err(conv(format!(
            "{what}: expected {want} byte(s), got {}",
            bytes.len()
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// DPT 1.x — 1-bit boolean.
// ---------------------------------------------------------------------------

/// DPT 1.x — a single boolean bit (bit 0). Sub-types (1.001 switch, 1.008
/// up/down, 1.009 open/close, …) share this encoding; the meaning of `true`
/// is given by the sub-type, which the codec is agnostic to.
pub mod dpt1 {
    use super::{conv, Result};

    /// Encode a boolean to its 1-bit value (`0` or `1`), the small payload that
    /// rides inside the telegram's application byte.
    #[must_use]
    pub const fn encode(value: bool) -> u8 {
        value as u8
    }

    /// Decode the 1-bit value back to a boolean. Only bit 0 is significant;
    /// any other set bit is rejected as malformed.
    ///
    /// # Errors
    /// Returns a conversion error if bits above bit 0 are set.
    pub fn decode(byte: u8) -> Result<bool> {
        match byte {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(conv(format!("boolean must be 0 or 1, got {other}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// DPT 2.x — 1-bit value with a control flag.
// ---------------------------------------------------------------------------

/// DPT 2.x — a 1-bit value plus a 1-bit "control" flag, packed into 2 bits:
/// `bit1 = control`, `bit0 = value`. When `control` is `false` the value is a
/// non-binding suggestion; when `true` it is a priority command.
pub mod dpt2 {
    use super::{conv, Result};

    /// A controlled boolean: the underlying on/off plus whether it is a
    /// priority ("control") command.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Controlled {
        /// `true` = priority/forced; `false` = advisory.
        pub control: bool,
        /// The boolean value itself.
        pub value: bool,
    }

    /// Encode into the 2-bit field.
    #[must_use]
    pub const fn encode(c: Controlled) -> u8 {
        ((c.control as u8) << 1) | (c.value as u8)
    }

    /// Decode from the 2-bit field. Bits above bit 1 must be clear.
    ///
    /// # Errors
    /// Returns a conversion error if bits above bit 1 are set.
    pub fn decode(byte: u8) -> Result<Controlled> {
        if byte > 0b11 {
            return Err(conv(format!("controlled bit uses only 2 bits, got {byte}")));
        }
        Ok(Controlled {
            control: (byte & 0b10) != 0,
            value: (byte & 0b01) != 0,
        })
    }
}

// ---------------------------------------------------------------------------
// DPT 3.x — 4-bit dimming / blind control.
// ---------------------------------------------------------------------------

/// DPT 3.x — 4-bit relative control for dimming (3.007) and blinds (3.008):
/// `bit3 = direction` (1 = up/brighter, 0 = down/darker), `bits2..0 = step
/// code` (0 = break/stop, 1..=7 = step intervals).
pub mod dpt3 {
    use super::{conv, Result};

    /// Travel direction of a relative control step.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Direction {
        /// Brighter, or blind up.
        Up,
        /// Darker, or blind down.
        Down,
    }

    /// A 3-bit step control: a direction plus a step code. Step code `0` is the
    /// "break"/stop command (direction is then irrelevant).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Control {
        /// Direction of travel.
        pub direction: Direction,
        /// Step code: `0` stops, `1..=7` are progressively smaller intervals.
        pub step_code: u8,
    }

    impl Control {
        /// `true` when this is the stop ("break") command.
        #[must_use]
        pub const fn is_stop(self) -> bool {
            self.step_code == 0
        }
    }

    /// Encode into the 4-bit field.
    ///
    /// # Errors
    /// Returns a conversion error if `step_code > 7`.
    pub fn encode(c: Control) -> Result<u8> {
        if c.step_code > 7 {
            return Err(conv(format!(
                "step code uses 3 bits (0..=7), got {}",
                c.step_code
            )));
        }
        let dir_bit = match c.direction {
            Direction::Up => 1u8,
            Direction::Down => 0u8,
        };
        Ok((dir_bit << 3) | c.step_code)
    }

    /// Decode from the 4-bit field. Bits above bit 3 must be clear.
    ///
    /// # Errors
    /// Returns a conversion error if bits above bit 3 are set.
    pub fn decode(byte: u8) -> Result<Control> {
        if byte > 0b1111 {
            return Err(conv(format!("control uses 4 bits, got {byte}")));
        }
        Ok(Control {
            direction: if byte & 0b1000 != 0 {
                Direction::Up
            } else {
                Direction::Down
            },
            step_code: byte & 0b0111,
        })
    }
}

// ---------------------------------------------------------------------------
// DPT 5.x — 1-byte unsigned.
// ---------------------------------------------------------------------------

/// DPT 5.x — 1-byte unsigned values with three common readings:
/// scaling (5.001, `0..=100 %`), angle (5.003, `0..=360°`), and raw counter
/// (5.010, `0..=255`). Scaling/angle map a real range linearly across `0..=255`
/// with round-half-up rounding (matching ETS / xknx).
pub mod dpt5 {
    use super::{conv, Result};

    /// DPT 5.001 — encode a percentage `0.0..=100.0` to a byte.
    ///
    /// `round(value / 100 * 255)`. So `50 %` → `128` (`127.5` rounds up).
    ///
    /// # Errors
    /// Returns a conversion error if `value` is outside `0.0..=100.0` or NaN.
    pub fn encode_scaling(value: f64) -> Result<u8> {
        encode_ranged(value, 100.0, "percentage")
    }

    /// DPT 5.001 — decode a byte back to a `0.0..=100.0` percentage.
    #[must_use]
    pub fn decode_scaling(byte: u8) -> f64 {
        decode_ranged(byte, 100.0)
    }

    /// DPT 5.003 — encode an angle `0.0..=360.0` to a byte.
    ///
    /// # Errors
    /// Returns a conversion error if `value` is outside `0.0..=360.0` or NaN.
    pub fn encode_angle(value: f64) -> Result<u8> {
        encode_ranged(value, 360.0, "angle")
    }

    /// DPT 5.003 — decode a byte back to a `0.0..=360.0` angle.
    #[must_use]
    pub fn decode_angle(byte: u8) -> f64 {
        decode_ranged(byte, 360.0)
    }

    /// DPT 5.010 — the raw byte counter, identity.
    #[must_use]
    pub const fn encode_raw(value: u8) -> u8 {
        value
    }

    /// DPT 5.010 — decode the raw byte counter, identity.
    #[must_use]
    pub const fn decode_raw(byte: u8) -> u8 {
        byte
    }

    fn encode_ranged(value: f64, top: f64, what: &str) -> Result<u8> {
        if value.is_nan() || !(0.0..=top).contains(&value) {
            return Err(conv(format!("{what} {value} out of range (0..={top})")));
        }
        Ok((value / top * 255.0).round() as u8)
    }

    fn decode_ranged(byte: u8, top: f64) -> f64 {
        f64::from(byte) / 255.0 * top
    }
}

// ---------------------------------------------------------------------------
// DPT 6.x — 1-byte signed.
// ---------------------------------------------------------------------------

/// DPT 6.x — 1-byte signed integer (two's complement, `-128..=127`).
pub mod dpt6 {
    use super::{expect_len, Result};

    /// Encode a signed 8-bit value.
    #[must_use]
    pub const fn encode(value: i8) -> [u8; 1] {
        [value as u8]
    }

    /// Decode a 1-byte payload to a signed 8-bit value.
    ///
    /// # Errors
    /// Returns a conversion error if the payload is not exactly 1 byte.
    pub fn decode(bytes: &[u8]) -> Result<i8> {
        expect_len(bytes, 1, "8-bit signed")?;
        Ok(bytes[0] as i8)
    }
}

// ---------------------------------------------------------------------------
// DPT 7.x / 8.x — 2-byte integers.
// ---------------------------------------------------------------------------

/// DPT 7.x — 2-byte unsigned integer, big-endian (`0..=65535`).
pub mod dpt7 {
    use super::{expect_len, Result};

    /// Encode a `u16` big-endian.
    #[must_use]
    pub const fn encode(value: u16) -> [u8; 2] {
        value.to_be_bytes()
    }

    /// Decode 2 big-endian bytes to a `u16`.
    ///
    /// # Errors
    /// Returns a conversion error if the payload is not exactly 2 bytes.
    pub fn decode(bytes: &[u8]) -> Result<u16> {
        expect_len(bytes, 2, "16-bit unsigned")?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }
}

/// DPT 8.x — 2-byte signed integer, big-endian (`-32768..=32767`).
pub mod dpt8 {
    use super::{expect_len, Result};

    /// Encode an `i16` big-endian.
    #[must_use]
    pub const fn encode(value: i16) -> [u8; 2] {
        value.to_be_bytes()
    }

    /// Decode 2 big-endian bytes to an `i16`.
    ///
    /// # Errors
    /// Returns a conversion error if the payload is not exactly 2 bytes.
    pub fn decode(bytes: &[u8]) -> Result<i16> {
        expect_len(bytes, 2, "16-bit signed")?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }
}

// ---------------------------------------------------------------------------
// DPT 9.x — the KNX 2-byte float. The tricky one.
// ---------------------------------------------------------------------------

/// DPT 9.x — the KNX 16-bit floating point, used for temperatures (9.001),
/// humidity (9.007), lux (9.004) and more.
///
/// Big-endian layout:
///
/// ```text
///   bit 15 | bits 14..11 | bits 10..0
///   sign   | exponent E  | mantissa M (11-bit, two's-complement together with sign)
/// ```
///
/// The encoded value is `value = 0.01 * M * 2^E`, where `M` is the signed
/// 12-bit quantity formed by the sign bit and the 11 mantissa bits. The full
/// representable range is roughly `-671088.64 ..= 670760.96`.
pub mod dpt9 {
    use super::{conv, expect_len, Result};

    /// The smallest value DPT 9 can represent.
    pub const MIN: f64 = -671_088.64;
    /// The largest value DPT 9 can represent.
    pub const MAX: f64 = 670_760.96;

    /// Encode an `f64` to the 2-byte KNX float.
    ///
    /// The value is scaled by 100, then the smallest exponent `E` (0..=15) is
    /// chosen that fits the scaled value into the signed 11-bit mantissa range
    /// `-2048..=2047`.
    ///
    /// # Errors
    /// Returns a conversion error if `value` is NaN, infinite, or outside
    /// `MIN..=MAX`.
    pub fn encode(value: f64) -> Result<[u8; 2]> {
        if value.is_nan() || value.is_infinite() {
            return Err(conv("value is not finite"));
        }
        if !(MIN..=MAX).contains(&value) {
            return Err(conv(format!("value {value} out of range ({MIN}..={MAX})")));
        }

        // Work on the value scaled by 100, rounded to the nearest integer.
        let mut scaled = (value * 100.0).round();
        // Exact zero (and anything that rounds to zero) encodes as 0x0000.
        if scaled == 0.0 {
            return Ok([0, 0]);
        }

        let mut exponent: i32 = 0;
        // Halve until the magnitude fits the signed 11-bit mantissa window.
        // Re-round each step so the integer mantissa we finally take is the
        // nearest representable one.
        while !(-2048.0..=2047.0).contains(&scaled) {
            scaled = (scaled / 2.0).round();
            exponent += 1;
            if exponent > 15 {
                return Err(conv(format!("value {value} cannot be represented")));
            }
        }

        let mantissa = scaled as i32; // in -2048..=2047
        let sign_bit: u16 = if mantissa < 0 { 0x8000 } else { 0 };
        // Take the low 11 bits of the two's-complement mantissa.
        let mant_bits = (mantissa as u16) & 0x07FF;
        let exp_bits = ((exponent as u16) & 0x0F) << 11;
        let word = sign_bit | exp_bits | mant_bits;
        Ok(word.to_be_bytes())
    }

    /// Decode a 2-byte KNX float to an `f64`.
    ///
    /// # Errors
    /// Returns a conversion error if the payload is not exactly 2 bytes.
    pub fn decode(bytes: &[u8]) -> Result<f64> {
        expect_len(bytes, 2, "KNX 2-byte float")?;
        let word = u16::from_be_bytes([bytes[0], bytes[1]]);
        let exponent = i32::from((word >> 11) & 0x0F);
        // Sign-extend the 12-bit (sign + 11) mantissa into an i32.
        let mut mantissa = i32::from(word & 0x07FF);
        if word & 0x8000 != 0 {
            mantissa -= 2048;
        }
        let value = f64::from(mantissa) * 0.01 * f64::from(1u32 << exponent);
        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// DPT 12.x / 13.x — 4-byte integers.
// ---------------------------------------------------------------------------

/// DPT 12.x — 4-byte unsigned integer, big-endian.
pub mod dpt12 {
    use super::{expect_len, Result};

    /// Encode a `u32` big-endian.
    #[must_use]
    pub const fn encode(value: u32) -> [u8; 4] {
        value.to_be_bytes()
    }

    /// Decode 4 big-endian bytes to a `u32`.
    ///
    /// # Errors
    /// Returns a conversion error if the payload is not exactly 4 bytes.
    pub fn decode(bytes: &[u8]) -> Result<u32> {
        expect_len(bytes, 4, "32-bit unsigned")?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

/// DPT 13.x — 4-byte signed integer, big-endian.
pub mod dpt13 {
    use super::{expect_len, Result};

    /// Encode an `i32` big-endian.
    #[must_use]
    pub const fn encode(value: i32) -> [u8; 4] {
        value.to_be_bytes()
    }

    /// Decode 4 big-endian bytes to an `i32`.
    ///
    /// # Errors
    /// Returns a conversion error if the payload is not exactly 4 bytes.
    pub fn decode(bytes: &[u8]) -> Result<i32> {
        expect_len(bytes, 4, "32-bit signed")?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

// ---------------------------------------------------------------------------
// DPT 14.x — 4-byte IEEE float.
// ---------------------------------------------------------------------------

/// DPT 14.x — IEEE-754 single-precision float, big-endian.
pub mod dpt14 {
    use super::{conv, expect_len, Result};

    /// Encode an `f32` big-endian.
    ///
    /// # Errors
    /// Returns a conversion error if `value` is not finite (KNX carries only
    /// finite measurements).
    pub fn encode(value: f32) -> Result<[u8; 4]> {
        if !value.is_finite() {
            return Err(conv("value is not finite"));
        }
        Ok(value.to_be_bytes())
    }

    /// Decode 4 big-endian bytes to an `f32`.
    ///
    /// # Errors
    /// Returns a conversion error if the payload is not exactly 4 bytes, or the
    /// decoded value is not finite.
    pub fn decode(bytes: &[u8]) -> Result<f32> {
        expect_len(bytes, 4, "32-bit float")?;
        let v = f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if !v.is_finite() {
            return Err(conv("decoded value is not finite"));
        }
        Ok(v)
    }
}

// ---------------------------------------------------------------------------
// DPT 16.x — 14-byte string.
// ---------------------------------------------------------------------------

/// DPT 16.x — a fixed 14-byte character string (16.000 ASCII, 16.001 latin-1),
/// null-padded on the right.
pub mod dpt16 {
    use super::{conv, Result};

    /// The fixed payload width in bytes.
    pub const LEN: usize = 14;

    /// Encode a string into a fixed 14-byte, null-padded payload.
    ///
    /// # Errors
    /// Returns a conversion error if the text is longer than 14 bytes, or
    /// contains a byte outside latin-1 (`0..=255`, which all `u8` are — so the
    /// only real failure is over-length / an embedded NUL).
    pub fn encode(text: &str) -> Result<[u8; LEN]> {
        let raw = text.as_bytes();
        if raw.len() > LEN {
            return Err(conv(format!(
                "text is {} bytes, the limit is {LEN}",
                raw.len()
            )));
        }
        if raw.contains(&0) {
            return Err(conv("text contains an embedded NUL"));
        }
        let mut out = [0u8; LEN];
        out[..raw.len()].copy_from_slice(raw);
        Ok(out)
    }

    /// Decode a 14-byte payload to a trimmed string, stopping at the first NUL.
    ///
    /// # Errors
    /// Returns a conversion error if the payload is not exactly 14 bytes or the
    /// content is not valid UTF-8 (we accept ASCII / UTF-8; latin-1 high bytes
    /// that are not valid UTF-8 are rejected rather than silently mangled).
    pub fn decode(bytes: &[u8]) -> Result<String> {
        if bytes.len() != LEN {
            return Err(conv(format!(
                "string payload must be {LEN} bytes, got {}",
                bytes.len()
            )));
        }
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(LEN);
        core::str::from_utf8(&bytes[..end])
            .map(str::to_owned)
            .map_err(|_| conv("string payload is not valid text"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- DPT 1 ----------------------------------------------------------
    #[test]
    fn dpt1_roundtrip_and_reject() {
        assert_eq!(dpt1::encode(true), 1);
        assert_eq!(dpt1::encode(false), 0);
        assert_eq!(dpt1::decode(1).unwrap(), true);
        assert_eq!(dpt1::decode(0).unwrap(), false);
        assert!(dpt1::decode(2).is_err());
    }

    // ---- DPT 2 ----------------------------------------------------------
    #[test]
    fn dpt2_controlled_roundtrip() {
        for control in [false, true] {
            for value in [false, true] {
                let c = dpt2::Controlled { control, value };
                let byte = dpt2::encode(c);
                assert_eq!(dpt2::decode(byte).unwrap(), c);
            }
        }
        // priority "on" is 0b11.
        assert_eq!(
            dpt2::encode(dpt2::Controlled {
                control: true,
                value: true
            }),
            0b11
        );
        assert!(dpt2::decode(0b100).is_err());
    }

    // ---- DPT 3 ----------------------------------------------------------
    #[test]
    fn dpt3_dimming_direction_and_step() {
        use dpt3::{Control, Direction};
        let up = Control {
            direction: Direction::Up,
            step_code: 1,
        };
        let byte = dpt3::encode(up).unwrap();
        assert_eq!(byte, 0b1001); // direction bit set + step 1
        assert_eq!(dpt3::decode(byte).unwrap(), up);

        let down = Control {
            direction: Direction::Down,
            step_code: 7,
        };
        assert_eq!(dpt3::encode(down).unwrap(), 0b0111);

        let stop = dpt3::decode(0b1000).unwrap();
        assert!(stop.is_stop());
    }

    #[test]
    fn dpt3_rejects_over_range() {
        use dpt3::{Control, Direction};
        assert!(dpt3::encode(Control {
            direction: Direction::Up,
            step_code: 8
        })
        .is_err());
        assert!(dpt3::decode(0b10000).is_err());
    }

    // ---- DPT 5 ----------------------------------------------------------
    #[test]
    fn dpt5_scaling_endpoints_and_rounding() {
        assert_eq!(dpt5::encode_scaling(0.0).unwrap(), 0);
        assert_eq!(dpt5::encode_scaling(100.0).unwrap(), 255);
        // 50% -> 127.5 -> rounds half up to 128.
        assert_eq!(dpt5::encode_scaling(50.0).unwrap(), 128);
        // 1% -> 2.55 -> 3 (round, not truncate).
        assert_eq!(dpt5::encode_scaling(1.0).unwrap(), 3);
        // round-trip the byte endpoints.
        assert!((dpt5::decode_scaling(0) - 0.0).abs() < 1e-9);
        assert!((dpt5::decode_scaling(255) - 100.0).abs() < 1e-9);
        assert!((dpt5::decode_scaling(128) - 50.196_078).abs() < 1e-3);
    }

    #[test]
    fn dpt5_scaling_rejects_out_of_range() {
        assert!(dpt5::encode_scaling(-0.1).is_err());
        assert!(dpt5::encode_scaling(100.1).is_err());
        assert!(dpt5::encode_scaling(f64::NAN).is_err());
    }

    #[test]
    fn dpt5_angle_endpoints() {
        assert_eq!(dpt5::encode_angle(0.0).unwrap(), 0);
        assert_eq!(dpt5::encode_angle(360.0).unwrap(), 255);
        // 180 deg -> 127.5 -> 128.
        assert_eq!(dpt5::encode_angle(180.0).unwrap(), 128);
        assert!(dpt5::encode_angle(361.0).is_err());
    }

    #[test]
    fn dpt5_raw_identity() {
        assert_eq!(dpt5::encode_raw(200), 200);
        assert_eq!(dpt5::decode_raw(200), 200);
    }

    // ---- DPT 6 ----------------------------------------------------------
    #[test]
    fn dpt6_signed_roundtrip() {
        for v in [-128i8, -1, 0, 1, 127] {
            assert_eq!(dpt6::decode(&dpt6::encode(v)).unwrap(), v);
        }
        assert_eq!(dpt6::encode(-1), [0xFF]);
        assert!(dpt6::decode(&[1, 2]).is_err());
    }

    // ---- DPT 7 / 8 ------------------------------------------------------
    #[test]
    fn dpt7_unsigned16_roundtrip() {
        assert_eq!(dpt7::encode(0x1234), [0x12, 0x34]);
        assert_eq!(dpt7::decode(&[0x12, 0x34]).unwrap(), 0x1234);
        assert_eq!(dpt7::decode(&dpt7::encode(65535)).unwrap(), 65535);
        assert!(dpt7::decode(&[0x12]).is_err());
    }

    #[test]
    fn dpt8_signed16_roundtrip() {
        for v in [i16::MIN, -1, 0, 1, i16::MAX] {
            assert_eq!(dpt8::decode(&dpt8::encode(v)).unwrap(), v);
        }
        assert_eq!(dpt8::encode(-1), [0xFF, 0xFF]);
        assert!(dpt8::decode(&[0]).is_err());
    }

    // ---- DPT 9 (the float) ---------------------------------------------
    #[test]
    fn dpt9_zero() {
        assert_eq!(dpt9::encode(0.0).unwrap(), [0x00, 0x00]);
        assert!((dpt9::decode(&[0x00, 0x00]).unwrap()).abs() < 1e-9);
    }

    #[test]
    fn dpt9_known_temperature_21c() {
        // 21.0 deg C is the canonical worked example: 0x0C1A.
        let bytes = dpt9::encode(21.0).unwrap();
        assert_eq!(bytes, [0x0C, 0x1A], "21.0 C must encode to 0x0C1A");
        assert!((dpt9::decode(&bytes).unwrap() - 21.0).abs() < 1e-9);
    }

    #[test]
    fn dpt9_known_zero_point_zero_one() {
        // smallest step: 0.01 -> mantissa 1, exp 0 -> 0x0001.
        let bytes = dpt9::encode(0.01).unwrap();
        assert_eq!(bytes, [0x00, 0x01]);
        assert!((dpt9::decode(&bytes).unwrap() - 0.01).abs() < 1e-9);
    }

    #[test]
    fn dpt9_negative_values() {
        // -10.0 deg C, a frost set-point.
        let bytes = dpt9::encode(-10.0).unwrap();
        assert!((dpt9::decode(&bytes).unwrap() + 10.0).abs() < 0.01);
        // exact negative small value -0.01 -> mantissa -1.
        let small = dpt9::encode(-0.01).unwrap();
        assert!((dpt9::decode(&small).unwrap() + 0.01).abs() < 1e-9);
    }

    #[test]
    fn dpt9_large_value_uses_exponent() {
        // a high lux reading exercises a non-zero exponent path.
        let v = 50_000.0;
        let bytes = dpt9::encode(v).unwrap();
        let back = dpt9::decode(&bytes).unwrap();
        // representation is lossy at this magnitude; within one quantum step.
        assert!((back - v).abs() <= 40.96, "got {back}");
    }

    #[test]
    fn dpt9_roundtrip_sweep() {
        // The KNX float quantizes by 0.01 * 2^exp, so larger magnitudes carry a
        // coarser step. We tolerate one quantum (a tiny fraction of the value).
        for &v in &[-273.15, -40.0, -0.5, 0.0, 0.5, 21.5, 37.0, 100.0, 1000.0] {
            let bytes = dpt9::encode(v).unwrap();
            let back = dpt9::decode(&bytes).unwrap();
            let tolerance = (v.abs() * 0.001).max(0.01) + 0.16;
            assert!((back - v).abs() <= tolerance, "v={v} back={back}");
        }
    }

    #[test]
    fn dpt9_rejects_out_of_range_and_nan() {
        assert!(dpt9::encode(1_000_000.0).is_err());
        assert!(dpt9::encode(-1_000_000.0).is_err());
        assert!(dpt9::encode(f64::NAN).is_err());
        assert!(dpt9::encode(f64::INFINITY).is_err());
        assert!(dpt9::decode(&[0x00]).is_err());
    }

    // ---- DPT 12 / 13 ----------------------------------------------------
    #[test]
    fn dpt12_unsigned32_roundtrip() {
        assert_eq!(dpt12::decode(&dpt12::encode(0xDEAD_BEEF)).unwrap(), 0xDEAD_BEEF);
        assert!(dpt12::decode(&[1, 2, 3]).is_err());
    }

    #[test]
    fn dpt13_signed32_roundtrip() {
        for v in [i32::MIN, -1, 0, 1, i32::MAX] {
            assert_eq!(dpt13::decode(&dpt13::encode(v)).unwrap(), v);
        }
        assert!(dpt13::decode(&[1, 2, 3, 4, 5]).is_err());
    }

    // ---- DPT 14 ---------------------------------------------------------
    #[test]
    fn dpt14_float_roundtrip() {
        let v = core::f32::consts::PI;
        let bytes = dpt14::encode(v).unwrap();
        assert!((dpt14::decode(&bytes).unwrap() - v).abs() < 1e-6);
        assert_eq!(dpt14::decode(&dpt14::encode(-42.5).unwrap()).unwrap(), -42.5);
    }

    #[test]
    fn dpt14_rejects_nonfinite_and_bad_len() {
        assert!(dpt14::encode(f32::NAN).is_err());
        assert!(dpt14::encode(f32::INFINITY).is_err());
        assert!(dpt14::decode(&[1, 2, 3]).is_err());
    }

    // ---- DPT 16 ---------------------------------------------------------
    #[test]
    fn dpt16_string_roundtrip_and_padding() {
        let bytes = dpt16::encode("Kitchen").unwrap();
        assert_eq!(bytes.len(), 14);
        assert_eq!(&bytes[..7], b"Kitchen");
        assert_eq!(bytes[7], 0); // null-padded
        assert_eq!(dpt16::decode(&bytes).unwrap(), "Kitchen");
    }

    #[test]
    fn dpt16_rejects_overlong_and_bad_len() {
        assert!(dpt16::encode("this is far too long").is_err());
        assert!(dpt16::decode(&[0u8; 13]).is_err());
        // exactly 14 chars is fine.
        assert_eq!(dpt16::encode("ABCDEFGHIJKLMN").unwrap().len(), 14);
    }
}
