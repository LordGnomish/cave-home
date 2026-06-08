// SPDX-License-Identifier: Apache-2.0
//! The Z-Wave fixed-point value encoding shared by Multilevel Sensor, Meter and
//! Thermostat Setpoint.
//!
//! Several Command Classes encode a real-world number the same way: a single
//! **metadata byte** followed by 1, 2 or 4 octets of a big-endian **signed**
//! integer. The metadata byte packs three fields (per the Z-Wave Command Class
//! specification, "Encapsulated Command Encoding"):
//!
//! ```text
//!   bit 7 6 5 | 4 3 | 2 1 0
//!     precision | scale | size
//! ```
//!
//! - **size** (bits 0–2): number of value octets — 1, 2 or 4. Other widths are
//!   illegal.
//! - **scale** (bits 3–4): which unit scale the value is in (its meaning is
//!   Command-Class-specific — e.g. Celsius vs Fahrenheit for temperature).
//! - **precision** (bits 5–7): number of decimal digits. The transmitted
//!   integer is the real value multiplied by `10^precision`, so the real value
//!   is `raw / 10^precision`.
//!
//! Example: the bytes `0x22 0x00 0xF4` describe precision = 1, scale = 0,
//! size = 2, raw = 0x00F4 = 244, hence 24.4 (e.g. 24.4 °C).

// This module is, by definition, byte-level fixed-point arithmetic: it
// reinterprets octets as signed two's-complement integers and scales them to
// `f64`. Every cast below is deliberate and range-checked before it runs
// (encode bounds-checks against the target width; decode reads exactly the
// declared number of octets). The lossy/sign-change cast lints are therefore
// expected here and silenced with intent rather than worked around.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use crate::error::{ZwaveError, ZwaveResult};

/// A decoded fixed-point value plus the metadata needed to interpret its unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedPoint {
    /// The decoded real value (`raw / 10^precision`).
    pub value: f64,
    /// The unit-scale selector (0–3); its meaning is Command-Class specific.
    pub scale: u8,
    /// Decimal precision (number of fractional digits, 0–7).
    pub precision: u8,
    /// Number of value octets that followed the metadata byte (1, 2 or 4).
    pub size: u8,
}

/// Split a metadata byte into `(precision, scale, size)`.
#[must_use]
pub const fn split_meta(meta: u8) -> (u8, u8, u8) {
    let precision = (meta >> 5) & 0b0000_0111;
    let scale = (meta >> 3) & 0b0000_0011;
    let size = meta & 0b0000_0111;
    (precision, scale, size)
}

/// Compose a metadata byte from its three fields.
///
/// # Errors
/// Returns [`ZwaveError`] if `precision > 7`, `scale > 3`, or `size` is not one
/// of 1/2/4.
pub fn compose_meta(precision: u8, scale: u8, size: u8) -> ZwaveResult<u8> {
    if precision > 7 {
        return Err(ZwaveError::OutOfRange {
            field: "precision",
            value: u32::from(precision),
        });
    }
    if scale > 3 {
        return Err(ZwaveError::OutOfRange {
            field: "scale",
            value: u32::from(scale),
        });
    }
    if !matches!(size, 1 | 2 | 4) {
        return Err(ZwaveError::BadValueSize { size });
    }
    Ok((precision << 5) | (scale << 3) | size)
}

/// Decode a metadata byte + value octets into a [`FixedPoint`].
///
/// `bytes` must begin at the metadata byte; any trailing bytes beyond the value
/// are ignored (a Command Class may carry more fields after the value).
///
/// # Errors
/// - [`ZwaveError::Truncated`] if `bytes` is empty or shorter than `1 + size`.
/// - [`ZwaveError::BadValueSize`] if the size field is not 1, 2 or 4.
pub fn decode(bytes: &[u8]) -> ZwaveResult<FixedPoint> {
    let &meta = bytes.first().ok_or(ZwaveError::Truncated { need: 1, got: 0 })?;
    let (precision, scale, size) = split_meta(meta);
    if !matches!(size, 1 | 2 | 4) {
        return Err(ZwaveError::BadValueSize { size });
    }
    let size_us = size as usize;
    if bytes.len() < 1 + size_us {
        return Err(ZwaveError::Truncated {
            need: 1 + size_us,
            got: bytes.len(),
        });
    }
    let raw = &bytes[1..=size_us];
    // Big-endian, two's-complement signed.
    let signed: i64 = match size {
        1 => i64::from(raw[0] as i8),
        2 => i64::from(i16::from_be_bytes([raw[0], raw[1]])),
        // size == 4 by the match above.
        _ => i64::from(i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]])),
    };
    let divisor = 10f64.powi(i32::from(precision));
    Ok(FixedPoint {
        value: signed as f64 / divisor,
        scale,
        precision,
        size,
    })
}

/// Encode a real value into a metadata byte + big-endian signed value octets.
///
/// The caller chooses `precision`, `scale` and `size`. The value is multiplied
/// by `10^precision` and rounded to the nearest integer before being packed.
///
/// # Errors
/// - [`ZwaveError`] from [`compose_meta`] on illegal precision/scale/size.
/// - [`ZwaveError::OutOfRange`] if the scaled value does not fit the chosen
///   signed width.
pub fn encode(value: f64, precision: u8, scale: u8, size: u8) -> ZwaveResult<Vec<u8>> {
    let meta = compose_meta(precision, scale, size)?;
    let scaled = (value * 10f64.powi(i32::from(precision))).round();
    let mut out = Vec::with_capacity(1 + size as usize);
    out.push(meta);
    match size {
        1 => {
            if !(f64::from(i8::MIN)..=f64::from(i8::MAX)).contains(&scaled) {
                return Err(ZwaveError::OutOfRange {
                    field: "value",
                    value: scaled.abs() as u32,
                });
            }
            out.push(scaled as i8 as u8);
        }
        2 => {
            if !(f64::from(i16::MIN)..=f64::from(i16::MAX)).contains(&scaled) {
                return Err(ZwaveError::OutOfRange {
                    field: "value",
                    value: scaled.abs() as u32,
                });
            }
            out.extend_from_slice(&(scaled as i16).to_be_bytes());
        }
        // size == 4 (compose_meta rejected anything else).
        _ => {
            if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&scaled) {
                return Err(ZwaveError::OutOfRange {
                    field: "value",
                    value: scaled.abs() as u32,
                });
            }
            out.extend_from_slice(&(scaled as i32).to_be_bytes());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_meta_matches_spec_layout() {
        // precision=1 (0b001), scale=0 (0b00), size=2 (0b010) => 0b001_00_010 = 0x22
        assert_eq!(split_meta(0x22), (1, 0, 2));
        // precision=2, scale=1, size=4 => 0b010_01_100 = 0x4C
        assert_eq!(split_meta(0x4C), (2, 1, 4));
    }

    #[test]
    fn temperature_24_4_celsius() {
        // 0x22 = precision 1, scale 0 (Celsius), size 2; 0x00F4 = 244 => 24.4
        let fp = decode(&[0x22, 0x00, 0xF4]).expect("valid temperature");
        assert!((fp.value - 24.4).abs() < 1e-9);
        assert_eq!(fp.scale, 0);
        assert_eq!(fp.precision, 1);
        assert_eq!(fp.size, 2);
    }

    #[test]
    fn negative_temperature_two_complement() {
        // precision 1, size 2, raw 0xFF38 = -200 => -20.0
        let fp = decode(&[0x22, 0xFF, 0x38]).expect("valid negative temp");
        assert!((fp.value - (-20.0)).abs() < 1e-9);
    }

    #[test]
    fn humidity_single_byte_no_fraction() {
        // precision 0, scale 0, size 1, raw 0x2A = 42 => 42 %
        let fp = decode(&[0x01, 0x2A]).expect("valid humidity");
        assert!((fp.value - 42.0).abs() < 1e-9);
        assert_eq!(fp.precision, 0);
        assert_eq!(fp.size, 1);
    }

    #[test]
    fn luminance_four_byte() {
        // precision 0, scale 1 (lux), size 4, raw 0x0001_86A0 = 100000 => 100000 lux
        let fp = decode(&[0x0C, 0x00, 0x01, 0x86, 0xA0]).expect("valid luminance");
        assert!((fp.value - 100_000.0).abs() < 1e-9);
        assert_eq!(fp.scale, 1);
        assert_eq!(fp.size, 4);
    }

    #[test]
    fn rejects_empty_and_truncated() {
        assert_eq!(decode(&[]), Err(ZwaveError::Truncated { need: 1, got: 0 }));
        // size 2 announced but only one value byte present.
        assert_eq!(
            decode(&[0x22, 0x00]),
            Err(ZwaveError::Truncated { need: 3, got: 2 })
        );
    }

    #[test]
    fn rejects_illegal_size() {
        // size field = 3 (0b011) is not 1/2/4.
        assert_eq!(decode(&[0x03, 0x00, 0x00, 0x00]), Err(ZwaveError::BadValueSize { size: 3 }));
    }

    #[test]
    fn encode_then_decode_roundtrips() {
        let bytes = encode(24.4, 1, 0, 2).expect("encodable");
        assert_eq!(bytes, vec![0x22, 0x00, 0xF4]);
        let fp = decode(&bytes).expect("decodable");
        assert!((fp.value - 24.4).abs() < 1e-9);
    }

    #[test]
    fn encode_rejects_overflow() {
        // 5000 with precision 0 does not fit a signed byte.
        assert!(encode(5000.0, 0, 0, 1).is_err());
        // but fits two octets.
        assert!(encode(5000.0, 0, 0, 2).is_ok());
    }

    #[test]
    fn compose_meta_rejects_bad_fields() {
        assert!(compose_meta(8, 0, 2).is_err());
        assert!(compose_meta(0, 4, 2).is_err());
        assert_eq!(compose_meta(0, 0, 3), Err(ZwaveError::BadValueSize { size: 3 }));
    }
}
