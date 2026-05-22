// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Crate-wide error type.
//!
//! Designed against the public `zigbee-herdsman` API surface (which
//! distinguishes Adapter errors from ZCL errors); the variants here
//! are derived from the spec sections cave-home-zigbee implements.

use thiserror::Error;

/// Top-level zigbee error.
#[derive(Debug, Error)]
pub enum ZigbeeError {
    /// Underlying serial / network transport error.
    #[error("transport: {0}")]
    Transport(String),

    /// EZSP frame layer error (parse / unsupported command).
    #[error("ezsp: {0}")]
    Ezsp(String),

    /// deCONZ serial protocol error.
    #[error("deconz: {0}")]
    Deconz(String),

    /// ZCL frame decode / encode error.
    #[error("zcl: {0}")]
    Zcl(String),

    /// Network / routing layer error.
    #[error("network: {0}")]
    Network(String),

    /// Pairing flow error.
    #[error("pairing: {0}")]
    Pairing(String),

    /// Generic I/O wrapper.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Buffer too short for a documented frame.
    #[error("buffer too short (need {need}, have {have})")]
    Truncated { need: usize, have: usize },

    /// Frame integrity check failed (CRC, sequence, length).
    #[error("integrity check failed: {0}")]
    Integrity(&'static str),

    /// A required device / endpoint / group is unknown to the stack.
    #[error("unknown {kind}: {id}")]
    Unknown { kind: &'static str, id: String },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, ZigbeeError>;
