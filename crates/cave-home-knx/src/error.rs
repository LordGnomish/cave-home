// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Errors shared across the cave-home-knx crate.
//!
//! Ported from `xknx/exceptions` (XKNX/xknx@50fdf8af, MIT) line-by-line:
//! the upstream classes `CouldNotParseAddress`, `CouldNotParseKNXIP`,
//! `IncompleteKNXIPFrame`, `ConversionError`, `CouldNotParseCEMI`,
//! `UnsupportedCEMIMessage` map onto the variants below. Idiomatic Rust
//! `thiserror` enum, semantically equivalent.

use thiserror::Error;

/// All errors the crate can raise.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum KnxError {
    /// Address string / int could not be parsed (`CouldNotParseAddress`).
    #[error("could not parse address: {0}")]
    AddressParse(String),

    /// KNX/IP frame is structurally invalid (`CouldNotParseKNXIP`).
    #[error("could not parse KNX/IP frame: {0}")]
    KnxIpParse(String),

    /// KNX/IP frame is shorter than expected (`IncompleteKNXIPFrame`).
    #[error("incomplete KNX/IP frame: {0}")]
    IncompleteFrame(String),

    /// CEMI frame is structurally invalid (`CouldNotParseCEMI`).
    #[error("could not parse CEMI frame: {0}")]
    CemiParse(String),

    /// CEMI variant not supported by this port (`UnsupportedCEMIMessage`).
    #[error("unsupported CEMI message: {0}")]
    UnsupportedCemi(String),

    /// DPT value out of range / wrong shape (`ConversionError`).
    #[error("DPT conversion failed: {0}")]
    Conversion(String),
}

/// Crate result alias.
pub type Result<T> = core::result::Result<T, KnxError>;
