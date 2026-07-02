// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Protobuf base-128 (LEB128) unsigned varint.
//!
//! This is the integer encoding the `ESPHome` native API uses for the frame's
//! payload-length and message-type prefixes (and, inside the payload, for every
//! protobuf field). Little-endian groups of 7 bits, the high bit of each byte
//! signalling "more bytes follow". A `u32` needs at most 5 bytes.

use crate::EsphomeError;

/// Append the base-128 varint encoding of `value` to `out`.
pub fn encode(value: u32, out: &mut Vec<u8>) {
    let mut v = value;
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Number of bytes [`encode`] appends for `value` — always `1..=5`.
#[must_use]
pub const fn encoded_len(value: u32) -> usize {
    match value {
        0..=0x7f => 1,
        0x80..=0x3fff => 2,
        0x4000..=0x1f_ffff => 3,
        0x20_0000..=0xfff_ffff => 4,
        _ => 5,
    }
}

/// Decode a base-128 varint from the front of `buf`.
///
/// * `Ok(Some((value, consumed)))` — a complete varint occupying `consumed`
///   leading bytes.
/// * `Ok(None)` — `buf` does not yet hold a complete varint; read more bytes
///   and retry. Running out of bytes mid-varint is a normal streaming state,
///   not an error.
/// * `Err(VarintOverflow)` — the varint would not fit in a `u32`.
///
/// # Errors
///
/// [`EsphomeError::VarintOverflow`] if the encoded value exceeds 32 bits.
pub fn decode(buf: &[u8]) -> Result<Option<(u32, usize)>, EsphomeError> {
    let mut result: u32 = 0;
    for (i, &byte) in buf.iter().enumerate().take(5) {
        // The 5th byte (i == 4) contributes bits 28..=34; only the low 4 are
        // valid for a u32, so anything above 0x0f (data bits) overflows — and a
        // continuation bit there means a 6th byte, which also overflows.
        if i == 4 && byte > 0x0f {
            return Err(EsphomeError::VarintOverflow);
        }
        result |= u32::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(Some((result, i + 1)));
        }
    }
    // A buffer of >= 5 bytes always returns inside the loop (the i == 4 byte is
    // either a terminator or an overflow), so reaching here means we ran out of
    // bytes mid-varint: incomplete.
    Ok(None)
}
