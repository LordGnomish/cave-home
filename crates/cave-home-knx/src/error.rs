// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Errors shared across the cave-home-knx crate.
//!
//! A small, std-only error enum. No `thiserror` dependency: the Phase-1 MVP is
//! deliberately dependency-free (std-only), so we hand-write [`core::fmt::Display`]
//! and implement [`std::error::Error`]. The variants mirror the failure modes a
//! pure-logic KNX codec actually has — address parsing, datapoint conversion,
//! and telegram framing — and the crate *never* panics on bad input: every
//! fallible path returns [`Result`].

use core::fmt;

/// All errors the crate can raise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnxError {
    /// An address string or raw value could not be parsed or was out of range.
    AddressParse(String),

    /// A datapoint value was out of range, or the raw payload had the wrong
    /// length / shape for the datapoint type.
    Conversion(String),

    /// A group telegram could not be parsed (too short, bad framing, or an
    /// application-service code this Phase-1 codec does not model).
    Telegram(String),
}

impl fmt::Display for KnxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AddressParse(m) => write!(f, "could not parse address: {m}"),
            Self::Conversion(m) => write!(f, "value conversion failed: {m}"),
            Self::Telegram(m) => write!(f, "could not parse telegram: {m}"),
        }
    }
}

impl std::error::Error for KnxError {}

/// Crate result alias.
pub type Result<T> = core::result::Result<T, KnxError>;
