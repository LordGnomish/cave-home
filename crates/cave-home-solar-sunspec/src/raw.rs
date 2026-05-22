// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Low-level register decoding helpers — `u16`/`i16`/`u32`/`i32`/string
//! reads on the SunSpec big-endian Modbus byte layout.
//!
//! SunSpec spec §B.2: all multi-register fields are stored in
//! big-endian (most significant register first). String fields are
//! null-padded ASCII.

use crate::error::{Error, Result};

/// Read a single `uint16` from a Modbus register vector at `offset`.
/// Returns `None` if the register carries the not-implemented sentinel
/// `0xFFFF`. Source: SunSpec spec §B.2 "not implemented" table.
#[must_use]
pub fn read_u16(regs: &[u16], offset: usize) -> Option<u16> {
    let v = regs.get(offset).copied()?;
    if v == 0xFFFF { None } else { Some(v) }
}

/// Read a single `int16`. Not-implemented sentinel is `0x8000`
/// (i.e. `i16::MIN`).
#[must_use]
pub fn read_i16(regs: &[u16], offset: usize) -> Option<i16> {
    let v = regs.get(offset).copied()?;
    let s = v as i16;
    if s == i16::MIN { None } else { Some(s) }
}

/// Read two consecutive registers as a 32-bit unsigned big-endian
/// integer. Returns `None` if both registers are `0xFFFF`
/// (not-implemented sentinel for uint32). Source: SunSpec spec §B.2.
#[must_use]
pub fn read_u32(regs: &[u16], offset: usize) -> Option<u32> {
    let hi = regs.get(offset).copied()?;
    let lo = regs.get(offset + 1).copied()?;
    if hi == 0xFFFF && lo == 0xFFFF {
        return None;
    }
    Some((u32::from(hi) << 16) | u32::from(lo))
}

/// Read two consecutive registers as a 32-bit signed big-endian
/// integer. Not-implemented sentinel is `0x8000_0000`.
#[must_use]
pub fn read_i32(regs: &[u16], offset: usize) -> Option<i32> {
    let hi = regs.get(offset).copied()?;
    let lo = regs.get(offset + 1).copied()?;
    let val = ((u32::from(hi) << 16) | u32::from(lo)) as i32;
    if val == i32::MIN { None } else { Some(val) }
}

/// Read two consecutive registers as a 32-bit "accumulator" (acc32).
/// acc32 sentinel for not-implemented is `0x0000_0000` per spec.
#[must_use]
pub fn read_acc32(regs: &[u16], offset: usize) -> Option<u32> {
    let hi = regs.get(offset).copied()?;
    let lo = regs.get(offset + 1).copied()?;
    let v = (u32::from(hi) << 16) | u32::from(lo);
    if v == 0 { None } else { Some(v) }
}

/// Read a SunSpec ASCII string of `register_count` 16-bit words
/// (`register_count × 2` bytes). Trailing nulls and whitespace are
/// stripped. Source: SunSpec spec §B.2.4.
///
/// # Errors
///
/// Returns [`Error::InvalidString`] if the decoded byte slice is not
/// valid UTF-8.
pub fn read_string(regs: &[u16], offset: usize, register_count: usize, field: &'static str) -> Result<String> {
    if offset + register_count > regs.len() {
        return Err(Error::ShortRead {
            expected: register_count as u16,
            actual: regs.len().saturating_sub(offset) as u16,
        });
    }
    let mut bytes = Vec::with_capacity(register_count * 2);
    for r in &regs[offset..offset + register_count] {
        bytes.push((r >> 8) as u8);
        bytes.push((r & 0xFF) as u8);
    }
    // Strip trailing NULs and ASCII whitespace.
    while matches!(bytes.last(), Some(0) | Some(b' ')) {
        bytes.pop();
    }
    String::from_utf8(bytes).map_err(|_| Error::InvalidString(field))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_u16_basic() {
        let regs = [0x1234u16, 0x5678];
        assert_eq!(read_u16(&regs, 0), Some(0x1234));
        assert_eq!(read_u16(&regs, 1), Some(0x5678));
        assert_eq!(read_u16(&regs, 2), None);
    }

    #[test]
    fn read_u16_detects_sentinel() {
        let regs = [0xFFFFu16];
        assert_eq!(read_u16(&regs, 0), None);
    }

    #[test]
    fn read_i16_handles_sign_and_sentinel() {
        let regs = [0xFFFEu16, 0x8000];
        assert_eq!(read_i16(&regs, 0), Some(-2));
        assert_eq!(read_i16(&regs, 1), None); // sentinel
    }

    #[test]
    fn read_u32_big_endian() {
        let regs = [0x1234u16, 0x5678];
        assert_eq!(read_u32(&regs, 0), Some(0x1234_5678));
    }

    #[test]
    fn read_u32_sentinel() {
        let regs = [0xFFFFu16, 0xFFFF];
        assert_eq!(read_u32(&regs, 0), None);
    }

    #[test]
    fn read_i32_handles_negative() {
        // 0xFFFF_FFFF == -1 i32
        let regs = [0xFFFFu16, 0xFFFE];
        assert_eq!(read_i32(&regs, 0), Some(-2));
    }

    #[test]
    fn read_i32_sentinel_min() {
        let regs = [0x8000u16, 0x0000];
        assert_eq!(read_i32(&regs, 0), None);
    }

    #[test]
    fn read_acc32_zero_is_not_implemented() {
        let regs = [0x0000u16, 0x0000];
        assert_eq!(read_acc32(&regs, 0), None);
    }

    #[test]
    fn read_acc32_non_zero_value() {
        let regs = [0x0001u16, 0x0002];
        assert_eq!(read_acc32(&regs, 0), Some(0x0001_0002));
    }

    #[test]
    fn read_string_round_trip() {
        // "Fronius" + NUL pad in 4 registers (8 bytes).
        let regs = [
            (b'F' as u16) << 8 | b'r' as u16,
            (b'o' as u16) << 8 | b'n' as u16,
            (b'i' as u16) << 8 | b'u' as u16,
            (b's' as u16) << 8,
        ];
        let s = read_string(&regs, 0, 4, "manufacturer").unwrap();
        assert_eq!(s, "Fronius");
    }

    #[test]
    fn read_string_short_read_errors() {
        let regs = [0x1234u16];
        assert!(read_string(&regs, 0, 4, "mfr").is_err());
    }
}
