// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Error type for the `ESPHome` native-API codec.

use core::fmt;

/// A decode/parse failure in the `ESPHome` native-API wire layer.
///
/// These are *protocol* errors — a frame that cannot be a valid plaintext
/// native-API frame. Running out of bytes mid-frame is **not** an error: the
/// decoder reports that as [`crate::frame::FrameDecode::Incomplete`] so the
/// caller can read more from the socket and try again.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EsphomeError {
    /// The frame began with `0x01`, the indicator for a Noise-encrypted frame.
    /// Phase-1 ships the plaintext helper only; encryption is Phase-1b.
    EncryptedFrame,
    /// The frame began with a byte that is neither `0x00` (plaintext) nor
    /// `0x01` (Noise). The stream is not a native-API frame stream.
    BadPreamble(u8),
    /// A base-128 varint did not terminate within the 5 bytes that can hold a
    /// `u32`, i.e. it would overflow 32 bits. A malformed or oversized length /
    /// message-type field.
    VarintOverflow,
}

impl fmt::Display for EsphomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EncryptedFrame => f.write_str(
                "the device sent an encrypted (Noise) frame; this connection needs an encryption key",
            ),
            Self::BadPreamble(b) => {
                write!(f, "not an ESPHome connection (unexpected first byte 0x{b:02x})")
            }
            Self::VarintOverflow => f.write_str("malformed ESPHome frame (length field too large)"),
        }
    }
}

impl core::error::Error for EsphomeError {}

/// Convenience alias for results in this crate.
pub type Result<T> = core::result::Result<T, EsphomeError>;
