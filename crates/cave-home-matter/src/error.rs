// SPDX-License-Identifier: Apache-2.0
//! Error type for cave-home-matter.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/lib/core/CHIPError.h
//!
//! Upstream uses a single `CHIP_ERROR` integer with thousands of
//! `CHIP_ERROR_FOO` constants. We adopt a Rust enum that splits by
//! subsystem; the discriminant numbers are recorded against the
//! upstream codes where they map directly.

use thiserror::Error;

/// Top-level Matter error.
///
/// # Upstream: src/lib/core/CHIPError.h::CHIP_ERROR
#[derive(Debug, Error)]
pub enum MatterError {
    /// `CHIP_ERROR_INVALID_ARGUMENT` (= 0x2f).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// `CHIP_ERROR_INVALID_MESSAGE_LENGTH` (= 0x18).
    #[error("invalid message length")]
    InvalidMessageLength,

    /// `CHIP_ERROR_INTERNAL` (= 0xac).
    #[error("internal error: {0}")]
    Internal(String),

    /// `CHIP_ERROR_KEY_NOT_FOUND` (= 0x37).
    #[error("not found: {0}")]
    NotFound(String),

    /// `CHIP_ERROR_DUPLICATE_KEY_ID` (= 0xa8).
    #[error("already exists: {0}")]
    AlreadyExists(String),

    /// `CHIP_ERROR_ACCESS_DENIED` (= 0xc5).
    #[error("access denied")]
    AccessDenied,

    /// `CHIP_ERROR_INCORRECT_STATE` (= 0x03).
    #[error("incorrect state: {0}")]
    IncorrectState(String),

    /// `CHIP_ERROR_TIMEOUT` (= 0x32).
    #[error("operation timed out")]
    Timeout,

    /// `CHIP_ERROR_INVALID_ADMIN_SUBJECT` etc. — admin/fabric errors.
    #[error("fabric error: {0}")]
    Fabric(String),

    /// PASE / CASE handshake failure.
    #[error("handshake failed: {0}")]
    Handshake(String),

    /// CHIP_ERROR_CRYPTO_ALGORITHM_NOT_SUPPORTED / verification failed.
    #[error("crypto error: {0}")]
    Crypto(String),

    /// Setup payload parse failure.
    #[error("setup payload parse error: {0}")]
    SetupPayloadParse(String),

    /// Transport / I/O failure.
    #[error("transport error: {0}")]
    Transport(String),
}

/// Convenience result alias.
pub type Result<T> = core::result::Result<T, MatterError>;
