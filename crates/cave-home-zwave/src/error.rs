// SPDX-License-Identifier: Apache-2.0
//! Crate-wide error type.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/core/src/error/ZWaveError.ts
//!
//! `ZWaveError` upstream is a class with a numeric `code` and a `message`; we
//! port the discriminants that the Phase 1 slice actually produces. The "long
//! tail" of upstream error codes (firmware update, NVM, OTA, …) will follow in
//! Phase 1b when their callers land.

use thiserror::Error;

/// The crate-wide error.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ZwaveError {
    /// A serial frame failed framing rules (truncated / bad SOF / etc.).
    ///
    /// Upstream: `ZWaveErrorCodes.PacketFormat_Invalid` /
    /// `PacketFormat_Truncated`.
    #[error("packet format invalid: {0}")]
    PacketFormat(String),

    /// A serial frame checksum did not match the computed XOR.
    ///
    /// Upstream: `ZWaveErrorCodes.PacketFormat_Checksum`.
    #[error("packet checksum mismatch (expected {expected:#04x}, got {got:#04x})")]
    PacketChecksum {
        /// Computed XOR over bytes 1..len-1.
        expected: u8,
        /// Value present in the last byte of the frame.
        got: u8,
    },

    /// An argument violated a precondition (length / value / range).
    ///
    /// Upstream: `ZWaveErrorCodes.Argument_Invalid`.
    #[error("invalid argument: {0}")]
    Argument(String),

    /// The transport's underlying I/O failed (UART hang-up, EOF, …).
    #[error("transport: {0}")]
    Transport(String),

    /// Driver / controller is not yet in the state required by the caller.
    #[error("driver not ready: {0}")]
    NotReady(String),

    /// Controller responded with an explicit failure.
    #[error("controller responded with failure: {0}")]
    Controller(String),

    /// An inclusion / exclusion run failed.
    #[error("inclusion failed: {0}")]
    Inclusion(String),

    /// Security S0 / S2 bootstrap failed.
    #[error("security: {0}")]
    Security(String),

    /// A node-targeted operation timed out.
    #[error("node {node_id} timed out after {millis} ms")]
    NodeTimeout {
        /// Node ID the operation targeted.
        node_id: u8,
        /// Milliseconds we waited.
        millis: u64,
    },
}

/// Crate-wide `Result` alias.
pub type ZwaveResult<T> = Result<T, ZwaveError>;
