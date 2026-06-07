// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Wire-codec and configuration errors.
//!
//! `CoreDNS` is infrastructure (Charter §6.3): these variants model the DNS
//! protocol's own failure vocabulary — truncation, malformed compression
//! pointers, oversized labels/names — for the codec and config layers. They are
//! never surfaced to the homeowner; a *protocol-level* refusal is expressed as
//! an [`crate::message`] `RCODE`, not as one of these errors.

use core::fmt;

/// The result type used throughout the crate's fallible codec/config paths.
pub type Result<T> = core::result::Result<T, WireError>;

/// A failure while decoding or encoding DNS wire data, or while parsing a
/// `Corefile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// The buffer ended before a fixed-size field could be read.
    UnexpectedEof {
        /// What the codec was trying to read when the buffer ran out.
        needed: &'static str,
    },
    /// A label length octet exceeds the RFC 1035 §2.3.4 limit of 63 octets.
    LabelTooLong {
        /// The offending label length.
        len: usize,
    },
    /// A name exceeds the RFC 1035 §2.3.4 limit of 255 octets on the wire.
    NameTooLong {
        /// The offending encoded length.
        len: usize,
    },
    /// A label contains an octet that is not permitted in a presentation name.
    InvalidLabel,
    /// A compression pointer pointed forward or to itself, risking a loop
    /// (RFC 1035 §4.1.4 pointers must reference a prior occurrence).
    BadCompressionPointer {
        /// The byte offset the pointer referenced.
        offset: usize,
    },
    /// The two high bits of a length octet were an undefined combination
    /// (`0b01` / `0b10` are reserved; only `0b00` label and `0b11` pointer
    /// are defined).
    ReservedLabelType,
    /// RDLENGTH disagreed with the bytes actually consumed by the RDATA codec.
    RdataLengthMismatch {
        /// The RDLENGTH the record declared.
        declared: usize,
        /// The number of octets the RDATA codec consumed.
        consumed: usize,
    },
    /// An RDATA field held a value the record type does not allow.
    InvalidRdata {
        /// A short, non-user-facing reason tag.
        reason: &'static str,
    },
    /// A `Corefile` was syntactically malformed.
    Corefile {
        /// A short, non-user-facing reason tag.
        reason: &'static str,
    },
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { needed } => write!(f, "unexpected end of buffer reading {needed}"),
            Self::LabelTooLong { len } => write!(f, "label too long: {len} > 63"),
            Self::NameTooLong { len } => write!(f, "name too long: {len} > 255"),
            Self::InvalidLabel => f.write_str("invalid label octet"),
            Self::BadCompressionPointer { offset } => {
                write!(f, "bad compression pointer to offset {offset}")
            }
            Self::ReservedLabelType => f.write_str("reserved label-type bits"),
            Self::RdataLengthMismatch { declared, consumed } => {
                write!(f, "rdlength mismatch: declared {declared}, consumed {consumed}")
            }
            Self::InvalidRdata { reason } => write!(f, "invalid rdata: {reason}"),
            Self::Corefile { reason } => write!(f, "corefile parse error: {reason}"),
        }
    }
}

impl core::error::Error for WireError {}
