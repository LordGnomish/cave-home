// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The free@home identifier scheme.
//!
//! Every addressable thing on a System Access Point is named by a small,
//! fixed-shape string. cave-home parses these into typed values once, at the
//! edge, so the rest of the engine never re-parses a raw string:
//!
//! - **Device serial** — e.g. `ABB700C12345`: a vendor prefix (letters/digits)
//!   followed by a hex tail. We accept the documented shape and keep the exact
//!   text for round-trip; serials are opaque identity, never arithmetic.
//! - **Channel id** — e.g. `ch0003`: the literal `ch` + a 4-hex-digit index.
//! - **Datapoint id** — e.g. `odp0000` (output) or `idp0001` (input): a 1-char
//!   direction prefix (`o`/`i`) + `dp` + a 4-hex-digit index.
//!
//! These identifiers are *developer-view* detail; they never reach a household
//! screen (Charter §6.3). They exist so the engine can address a datapoint
//! precisely.

use core::fmt;

/// Why a free@home identifier failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    /// The string was empty.
    Empty,
    /// The fixed prefix (`ch`, `dp`, `i`/`o`) was missing or wrong.
    BadPrefix,
    /// The numeric index part was not the expected 4 hex digits.
    BadIndex,
    /// A device serial contained characters outside the documented set.
    BadSerial,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("identifier is empty"),
            Self::BadPrefix => f.write_str("identifier has the wrong prefix"),
            Self::BadIndex => f.write_str("identifier index is not 4 hex digits"),
            Self::BadSerial => f.write_str("device serial has invalid characters"),
        }
    }
}

impl std::error::Error for IdError {}

/// A device serial such as `ABB700C12345`.
///
/// Serials are opaque identity. We validate the documented character set
/// (uppercase letters + digits, 6..=16 chars) and preserve the exact text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceSerial(String);

impl DeviceSerial {
    /// Parse and validate a device serial.
    pub fn parse(s: &str) -> Result<Self, IdError> {
        if s.is_empty() {
            return Err(IdError::Empty);
        }
        if s.len() < 6 || s.len() > 16 {
            return Err(IdError::BadSerial);
        }
        if !s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
            return Err(IdError::BadSerial);
        }
        Ok(Self(s.to_string()))
    }

    /// The serial as written on the device.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceSerial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A channel id such as `ch0003`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelId(u16);

impl ChannelId {
    /// Construct from a raw index.
    #[must_use]
    pub const fn new(index: u16) -> Self {
        Self(index)
    }

    /// Parse a `chXXXX` channel id (4 hex digits).
    pub fn parse(s: &str) -> Result<Self, IdError> {
        let rest = s.strip_prefix("ch").ok_or(IdError::BadPrefix)?;
        Ok(Self(parse_hex4(rest)?))
    }

    /// The numeric channel index.
    #[must_use]
    pub const fn index(self) -> u16 {
        self.0
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ch{:04x}", self.0)
    }
}

/// Whether a datapoint is an input (a command target) or an output (a state
/// the device reports).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// `idp…` — an input datapoint; you write commands to it.
    Input,
    /// `odp…` — an output datapoint; the device reports state on it.
    Output,
}

impl Direction {
    const fn prefix_char(self) -> char {
        match self {
            Self::Input => 'i',
            Self::Output => 'o',
        }
    }
}

/// A datapoint id such as `odp0000` (output) or `idp0001` (input).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DatapointId {
    direction: Direction,
    index: u16,
}

impl DatapointId {
    /// Construct from a direction + raw index.
    #[must_use]
    pub const fn new(direction: Direction, index: u16) -> Self {
        Self { direction, index }
    }

    /// Parse an `idpXXXX` / `odpXXXX` datapoint id (4 hex digits).
    pub fn parse(s: &str) -> Result<Self, IdError> {
        let mut chars = s.chars();
        let direction = match chars.next() {
            Some('i') => Direction::Input,
            Some('o') => Direction::Output,
            Some(_) => return Err(IdError::BadPrefix),
            None => return Err(IdError::Empty),
        };
        let rest = chars.as_str();
        let rest = rest.strip_prefix("dp").ok_or(IdError::BadPrefix)?;
        Ok(Self {
            direction,
            index: parse_hex4(rest)?,
        })
    }

    #[must_use]
    pub const fn direction(self) -> Direction {
        self.direction
    }

    #[must_use]
    pub const fn index(self) -> u16 {
        self.index
    }

    /// Whether this is an input (writable) datapoint.
    #[must_use]
    pub const fn is_input(self) -> bool {
        matches!(self.direction, Direction::Input)
    }
}

impl fmt::Display for DatapointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}dp{:04x}", self.direction.prefix_char(), self.index)
    }
}

/// Parse exactly 4 lowercase-or-uppercase hex digits into a `u16`.
fn parse_hex4(s: &str) -> Result<u16, IdError> {
    if s.len() != 4 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(IdError::BadIndex);
    }
    u16::from_str_radix(s, 16).map_err(|_| IdError::BadIndex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_round_trips() {
        let c = ChannelId::parse("ch0003").expect("valid");
        assert_eq!(c.index(), 3);
        assert_eq!(c.to_string(), "ch0003");
    }

    #[test]
    fn channel_high_index_round_trips() {
        let c = ChannelId::parse("ch00ff").expect("valid");
        assert_eq!(c.index(), 255);
        assert_eq!(c.to_string(), "ch00ff");
        assert_eq!(ChannelId::new(255).to_string(), "ch00ff");
    }

    #[test]
    fn channel_rejects_bad_prefix_and_index() {
        assert_eq!(ChannelId::parse("xx0003"), Err(IdError::BadPrefix));
        assert_eq!(ChannelId::parse("ch003"), Err(IdError::BadIndex));
        assert_eq!(ChannelId::parse("ch00zz"), Err(IdError::BadIndex));
        assert_eq!(ChannelId::parse("ch00003"), Err(IdError::BadIndex));
    }

    #[test]
    fn datapoint_output_round_trips() {
        let d = DatapointId::parse("odp0000").expect("valid");
        assert_eq!(d.direction(), Direction::Output);
        assert_eq!(d.index(), 0);
        assert!(!d.is_input());
        assert_eq!(d.to_string(), "odp0000");
    }

    #[test]
    fn datapoint_input_round_trips() {
        let d = DatapointId::parse("idp0001").expect("valid");
        assert_eq!(d.direction(), Direction::Input);
        assert_eq!(d.index(), 1);
        assert!(d.is_input());
        assert_eq!(d.to_string(), "idp0001");
    }

    #[test]
    fn datapoint_rejects_bad_inputs() {
        assert_eq!(DatapointId::parse(""), Err(IdError::Empty));
        assert_eq!(DatapointId::parse("xdp0000"), Err(IdError::BadPrefix));
        assert_eq!(DatapointId::parse("oxx0000"), Err(IdError::BadPrefix));
        assert_eq!(DatapointId::parse("odp00"), Err(IdError::BadIndex));
    }

    #[test]
    fn serial_parses_and_preserves_text() {
        let s = DeviceSerial::parse("ABB700C12345").expect("valid");
        assert_eq!(s.as_str(), "ABB700C12345");
        assert_eq!(s.to_string(), "ABB700C12345");
    }

    #[test]
    fn serial_rejects_lowercase_and_empty_and_symbols() {
        assert_eq!(DeviceSerial::parse(""), Err(IdError::Empty));
        assert_eq!(DeviceSerial::parse("abb700c12345"), Err(IdError::BadSerial));
        assert_eq!(DeviceSerial::parse("ABB-700"), Err(IdError::BadSerial));
        assert_eq!(DeviceSerial::parse("ABC"), Err(IdError::BadSerial));
    }
}
