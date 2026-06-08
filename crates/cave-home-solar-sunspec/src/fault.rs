// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Decode-failure type for the `SunSpec` register-map parser.
//!
//! Every decode path returns a `Result`; the parser never panics on a
//! malformed or truncated register block (Charter forbids `panic!` /
//! `unwrap` in shipped code). All variants are recoverable — a caller can
//! skip the offending model and carry on.

/// Why a SunSpec decode failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// A point read past the end of the supplied register slice. Carries the
    /// offending word offset and the slice length so a caller can log it
    /// (internally — never shown to the end-user).
    OutOfBounds { offset: usize, len: usize },
    /// A string point held bytes that were not valid UTF-8.
    InvalidString { offset: usize },
    /// A model declared more registers than the supplied block contains. A
    /// device that does this is misbehaving; we reject rather than read junk.
    LengthMismatch { model_id: u16, declared: u16, available: u16 },
    /// The SunSpec `"SunS"` identifier marker was not found at the block base.
    /// Either the wrong base register was probed or the device is not SunSpec.
    MissingMarker,
    /// A model id was asked of a decoder that does not understand it.
    UnsupportedModel { model_id: u16 },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfBounds { offset, len } => {
                write!(f, "register {offset} is past the end of a {len}-register block")
            }
            Self::InvalidString { offset } => {
                write!(f, "text field at register {offset} is not valid UTF-8")
            }
            Self::LengthMismatch { model_id, declared, available } => write!(
                f,
                "model {model_id} declares {declared} registers but only {available} are present"
            ),
            Self::MissingMarker => f.write_str("no SunSpec identifier marker at the probed base"),
            Self::UnsupportedModel { model_id } => {
                write!(f, "model {model_id} is not decoded by this crate")
            }
        }
    }
}

impl std::error::Error for DecodeError {}
