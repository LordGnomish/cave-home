// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec parser / reader error type.

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum Error {
    /// The SunSpec marker (`"SunS"` / `0x53756e53`) was not found at
    /// any of the well-known base registers (40000 / 50000 / 0).
    #[error("SunSpec marker not found at any well-known base register")]
    MarkerNotFound,

    /// A Modbus read returned fewer registers than expected for the
    /// declared model length.
    #[error("short read: expected {expected} registers, got {actual}")]
    ShortRead { expected: u16, actual: u16 },

    /// A required register inside a model carried the `not-implemented`
    /// sentinel (0x8000 for int16, 0xFFFF for uint16, etc.) where a
    /// value is mandatory. Source: SunSpec spec §B.2.
    #[error("required register `{0}` is marked not-implemented")]
    NotImplemented(&'static str),

    /// A model ID was read that is not in cave-home's supported set.
    /// The caller can choose to skip it.
    #[error("unsupported model id {0}")]
    UnsupportedModel(u16),

    /// The decoded length field doesn't match the on-the-wire length.
    #[error("model {model_id} declared length {declared}, payload was {actual}")]
    LengthMismatch {
        model_id: u16,
        declared: u16,
        actual: u16,
    },

    /// A scale factor was outside the legal `-10..=10` range. Source:
    /// SunSpec spec §B.3.
    #[error("scale factor {0} out of range (-10..=10)")]
    ScaleOutOfRange(i16),

    /// String-valued register block contained invalid UTF-8.
    #[error("non-UTF-8 SunSpec string in field `{0}`")]
    InvalidString(&'static str),

    /// Underlying Modbus transport error. Stringified to keep the
    /// type pure-Rust and avoid leaking transport types into the
    /// public surface.
    #[error("modbus transport error: {0}")]
    Transport(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_not_found_message() {
        assert_eq!(
            Error::MarkerNotFound.to_string(),
            "SunSpec marker not found at any well-known base register"
        );
    }

    #[test]
    fn short_read_carries_counts() {
        let e = Error::ShortRead {
            expected: 64,
            actual: 32,
        };
        assert!(e.to_string().contains("64"));
        assert!(e.to_string().contains("32"));
    }

    #[test]
    fn length_mismatch_carries_model_id() {
        let e = Error::LengthMismatch {
            model_id: 103,
            declared: 50,
            actual: 32,
        };
        assert!(e.to_string().contains("103"));
    }
}
