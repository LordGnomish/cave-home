// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec point-type decoders over a slice of Modbus holding registers.
//!
//! A SunSpec device exposes its data as a block of 16-bit holding registers.
//! When read over Modbus the values arrive big-endian, so a 32-bit point
//! spans two consecutive registers with the high word first. Each SunSpec
//! *point* has a declared type (`int16`, `uint16`, `int32`, `uint32`,
//! `acc32`, `float32`, `sunssf`, `string`); this module turns the raw `u16`
//! words into typed Rust values.
//!
//! # Sentinels
//!
//! SunSpec reserves a "not implemented" / "not accumulated" sentinel per
//! type so a device can advertise a point it does not actually populate.
//! Decoding a sentinel returns [`None`] — never a bogus number. Source:
//! SunSpec Information Model Specification, "Data Point Types" table.
//!
//! | Type     | Not-implemented sentinel |
//! | -------- | ------------------------ |
//! | int16    | `0x8000`                 |
//! | uint16   | `0xFFFF`                 |
//! | int32    | `0x8000_0000`            |
//! | uint32   | `0xFFFF_FFFF`            |
//! | acc32    | `0x0000_0000` (not accumulated) |
//! | sunssf   | `0x8000` (int16)         |
//!
//! `float32` uses IEEE-754 NaN as its not-implemented marker.

use crate::fault::DecodeError;

/// SunSpec `int16` not-implemented sentinel.
pub const INT16_NA: u16 = 0x8000;
/// SunSpec `uint16` not-implemented sentinel.
pub const UINT16_NA: u16 = 0xFFFF;
/// SunSpec `int32` not-implemented sentinel.
pub const INT32_NA: u32 = 0x8000_0000;
/// SunSpec `uint32` not-implemented sentinel.
pub const UINT32_NA: u32 = 0xFFFF_FFFF;
/// SunSpec `acc32` not-accumulated sentinel (a lifetime counter that has
/// never ticked reads as zero and carries no information).
pub const ACC32_NA: u32 = 0x0000_0000;

/// Read a single register as `int16`, honouring the `0x8000` sentinel.
///
/// # Errors
/// [`DecodeError::OutOfBounds`] if `offset` is past the end of `regs`.
pub fn int16(regs: &[u16], offset: usize) -> Result<Option<i16>, DecodeError> {
    let word = word_at(regs, offset)?;
    if word == INT16_NA {
        return Ok(None);
    }
    Ok(Some(word as i16))
}

/// Read a single register as `uint16`, honouring the `0xFFFF` sentinel.
///
/// # Errors
/// [`DecodeError::OutOfBounds`] if `offset` is past the end of `regs`.
pub fn uint16(regs: &[u16], offset: usize) -> Result<Option<u16>, DecodeError> {
    let word = word_at(regs, offset)?;
    if word == UINT16_NA {
        return Ok(None);
    }
    Ok(Some(word))
}

/// Read a `sunssf` scale-factor point (a signed `int16` exponent).
///
/// Shares the `int16` decode but exists as its own name so callers read as
/// the spec does. See [`crate::scale`] for applying the factor.
///
/// # Errors
/// [`DecodeError::OutOfBounds`] if `offset` is past the end of `regs`.
pub fn sunssf(regs: &[u16], offset: usize) -> Result<Option<i16>, DecodeError> {
    int16(regs, offset)
}

/// Read two consecutive registers as a big-endian `int32`.
///
/// # Errors
/// [`DecodeError::OutOfBounds`] if the pair runs off the end of `regs`.
pub fn int32(regs: &[u16], offset: usize) -> Result<Option<i32>, DecodeError> {
    let raw = u32_words(regs, offset)?;
    if raw == INT32_NA {
        return Ok(None);
    }
    Ok(Some(raw as i32))
}

/// Read two consecutive registers as a big-endian `uint32`.
///
/// # Errors
/// [`DecodeError::OutOfBounds`] if the pair runs off the end of `regs`.
pub fn uint32(regs: &[u16], offset: usize) -> Result<Option<u32>, DecodeError> {
    let raw = u32_words(regs, offset)?;
    if raw == UINT32_NA {
        return Ok(None);
    }
    Ok(Some(raw))
}

/// Read an `acc32` accumulator (e.g. lifetime energy). A value of zero means
/// "not accumulated" and decodes to [`None`].
///
/// # Errors
/// [`DecodeError::OutOfBounds`] if the pair runs off the end of `regs`.
pub fn acc32(regs: &[u16], offset: usize) -> Result<Option<u32>, DecodeError> {
    let raw = u32_words(regs, offset)?;
    if raw == ACC32_NA {
        return Ok(None);
    }
    Ok(Some(raw))
}

/// Read two consecutive registers as a big-endian IEEE-754 `float32`.
/// A NaN payload is the SunSpec not-implemented marker and decodes to
/// [`None`].
///
/// # Errors
/// [`DecodeError::OutOfBounds`] if the pair runs off the end of `regs`.
pub fn float32(regs: &[u16], offset: usize) -> Result<Option<f32>, DecodeError> {
    let raw = u32_words(regs, offset)?;
    let value = f32::from_bits(raw);
    if value.is_nan() {
        return Ok(None);
    }
    Ok(Some(value))
}

/// Decode a SunSpec fixed-length string point.
///
/// A string point occupies `len_regs` consecutive registers (`2 * len_regs`
/// bytes), MSB-first within each register. The string is NUL-padded; SunSpec
/// also pads with spaces in some implementations, so both trailing NULs and
/// trailing ASCII spaces are trimmed. An all-NUL field decodes to an empty
/// string.
///
/// # Errors
/// [`DecodeError::OutOfBounds`] if the field runs off the end of `regs`.
/// [`DecodeError::InvalidString`] if the bytes are not valid UTF-8.
pub fn string(regs: &[u16], offset: usize, len_regs: usize) -> Result<String, DecodeError> {
    let end = offset
        .checked_add(len_regs)
        .ok_or(DecodeError::OutOfBounds { offset, len: regs.len() })?;
    if end > regs.len() {
        return Err(DecodeError::OutOfBounds { offset: end, len: regs.len() });
    }
    let mut bytes = Vec::with_capacity(len_regs * 2);
    for &word in &regs[offset..end] {
        bytes.push((word >> 8) as u8);
        bytes.push((word & 0x00FF) as u8);
    }
    // Trim trailing NUL and space padding.
    while matches!(bytes.last(), Some(0 | b' ')) {
        bytes.pop();
    }
    String::from_utf8(bytes).map_err(|_| DecodeError::InvalidString { offset })
}

/// Fetch one register word, bounds-checked.
fn word_at(regs: &[u16], offset: usize) -> Result<u16, DecodeError> {
    regs.get(offset)
        .copied()
        .ok_or(DecodeError::OutOfBounds { offset, len: regs.len() })
}

/// Combine two consecutive registers into a big-endian `u32`.
fn u32_words(regs: &[u16], offset: usize) -> Result<u32, DecodeError> {
    let hi = word_at(regs, offset)?;
    let lo = word_at(regs, offset + 1)?;
    Ok((u32::from(hi) << 16) | u32::from(lo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int16_decodes_signed() {
        let regs = [0x0000, 0x7FFF, (-5i16) as u16];
        assert_eq!(int16(&regs, 0).unwrap(), Some(0));
        assert_eq!(int16(&regs, 1).unwrap(), Some(32_767));
        assert_eq!(int16(&regs, 2).unwrap(), Some(-5));
    }

    #[test]
    fn int16_sentinel_is_none() {
        let regs = [INT16_NA];
        assert_eq!(int16(&regs, 0).unwrap(), None);
    }

    #[test]
    fn uint16_decodes_and_sentinel() {
        let regs = [0u16, 1234, UINT16_NA];
        assert_eq!(uint16(&regs, 0).unwrap(), Some(0));
        assert_eq!(uint16(&regs, 1).unwrap(), Some(1234));
        assert_eq!(uint16(&regs, 2).unwrap(), None);
    }

    #[test]
    fn int32_big_endian_word_order() {
        // 0x000F_4240 == 1_000_000
        let regs = [0x000F, 0x4240];
        assert_eq!(int32(&regs, 0).unwrap(), Some(1_000_000));
    }

    #[test]
    fn int32_negative_and_sentinel() {
        let neg = (-2i32) as u32;
        let regs = [(neg >> 16) as u16, (neg & 0xFFFF) as u16];
        assert_eq!(int32(&regs, 0).unwrap(), Some(-2));
        let na = [0x8000u16, 0x0000];
        assert_eq!(int32(&na, 0).unwrap(), None);
    }

    #[test]
    fn uint32_decodes_and_sentinel() {
        let regs = [0xFFFF, 0xFFFE];
        assert_eq!(uint32(&regs, 0).unwrap(), Some(0xFFFF_FFFE));
        let na = [0xFFFFu16, 0xFFFF];
        assert_eq!(uint32(&na, 0).unwrap(), None);
    }

    #[test]
    fn acc32_zero_is_not_accumulated() {
        let zero = [0u16, 0];
        assert_eq!(acc32(&zero, 0).unwrap(), None);
        let some = [0x0001u16, 0x86A0]; // 100_000
        assert_eq!(acc32(&some, 0).unwrap(), Some(100_000));
    }

    #[test]
    fn float32_round_trips() {
        let bits = 230.5f32.to_bits();
        let regs = [(bits >> 16) as u16, (bits & 0xFFFF) as u16];
        let got = float32(&regs, 0).unwrap().expect("finite");
        assert!((got - 230.5).abs() < f32::EPSILON);
    }

    #[test]
    fn float32_nan_is_none() {
        let bits = f32::NAN.to_bits();
        let regs = [(bits >> 16) as u16, (bits & 0xFFFF) as u16];
        assert_eq!(float32(&regs, 0).unwrap(), None);
    }

    #[test]
    fn string_decodes_msb_first_and_trims_padding() {
        // "Fronius" packed two bytes per register, NUL padded to 4 regs.
        let regs = [
            (u16::from(b'F') << 8) | u16::from(b'r'),
            (u16::from(b'o') << 8) | u16::from(b'n'),
            (u16::from(b'i') << 8) | u16::from(b'u'),
            (u16::from(b's') << 8),
        ];
        assert_eq!(string(&regs, 0, 4).unwrap(), "Fronius");
    }

    #[test]
    fn string_trims_trailing_spaces() {
        let regs = [
            (u16::from(b'S') << 8) | u16::from(b'M'),
            (u16::from(b'A') << 8) | u16::from(b' '),
        ];
        assert_eq!(string(&regs, 0, 2).unwrap(), "SMA");
    }

    #[test]
    fn string_empty_field() {
        let regs = [0u16, 0, 0, 0];
        assert_eq!(string(&regs, 0, 4).unwrap(), "");
    }

    #[test]
    fn out_of_bounds_is_error_not_panic() {
        let regs = [1u16];
        assert!(matches!(int16(&regs, 5), Err(DecodeError::OutOfBounds { .. })));
        assert!(matches!(uint32(&regs, 0), Err(DecodeError::OutOfBounds { .. })));
        assert!(matches!(string(&regs, 0, 4), Err(DecodeError::OutOfBounds { .. })));
    }

    #[test]
    fn sunssf_is_int16_decode() {
        let regs = [(-2i16) as u16];
        assert_eq!(sunssf(&regs, 0).unwrap(), Some(-2));
    }
}
