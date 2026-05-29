// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! KNX address model — group addresses and individual (physical) addresses.
//!
//! Built from the public KNX address specification. Two address kinds share the
//! KNX bus, both carried on the wire as a big-endian `u16`:
//!
//! * [`GroupAddress`] — the *logical* destination an action targets (a light, a
//!   blind, a thermostat set-point). Written by installers in three notations:
//!   * 3-level `"main/middle/sub"` e.g. `"1/2/3"` (5 / 3 / 8 bits),
//!   * 2-level `"main/sub"` e.g. `"1/500"` (5 / 11 bits),
//!   * free `"12345"` (the bare 16-bit number).
//! * [`IndividualAddress`] — the *physical* device address `area.line.device`
//!   e.g. `"1.1.5"` (4 / 4 / 8 bits).
//!
//! Every type round-trips parse ⇄ `u16` ⇄ string and validates each field's
//! range, returning [`Result`] rather than panicking on malformed input.

use core::fmt;

use crate::error::{KnxError, Result};

/// The notation a [`GroupAddress`] is written and displayed in.
///
/// The on-the-wire `u16` is identical across notations; the notation only
/// changes how the number is split for humans.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GroupAddressStyle {
    /// Free format — the bare 16-bit number, e.g. `"12345"`.
    Free,
    /// 2-level `"main/sub"` — 5-bit main, 11-bit sub.
    TwoLevel,
    /// 3-level `"main/middle/sub"` — 5-bit main, 3-bit middle, 8-bit sub.
    /// The KNX default.
    #[default]
    ThreeLevel,
}

/// A KNX individual (physical) device address `area.line.device`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndividualAddress {
    raw: u16,
}

impl IndividualAddress {
    /// Highest valid area part (4 bits).
    pub const MAX_AREA: u16 = 15;
    /// Highest valid line part (4 bits).
    pub const MAX_LINE: u16 = 15;
    /// Highest valid device part (8 bits).
    pub const MAX_DEVICE: u16 = 255;

    /// Build from the raw 16-bit value. Every `u16` is a structurally valid
    /// individual address, so this cannot fail.
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        Self { raw }
    }

    /// Decode from the canonical 2-byte big-endian wire representation.
    #[must_use]
    pub const fn from_bytes(raw: [u8; 2]) -> Self {
        Self::from_raw(u16::from_be_bytes(raw))
    }

    /// Encode to the canonical 2-byte big-endian wire representation.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 2] {
        self.raw.to_be_bytes()
    }

    /// Parse `"area.line.device"` (e.g. `"1.1.5"`), or a bare `u16`.
    ///
    /// # Errors
    /// Returns [`KnxError::AddressParse`] if the shape is wrong, a part is not
    /// numeric, or any part is out of its range.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        if let Ok(n) = s.parse::<u16>() {
            return Ok(Self::from_raw(n));
        }
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(KnxError::AddressParse(format!(
                "expected area.line.device, got {s:?}"
            )));
        }
        let area = parse_part(parts[0], "area", s)?;
        let line = parse_part(parts[1], "line", s)?;
        let device = parse_part(parts[2], "device", s)?;
        range(area, Self::MAX_AREA, "area", s)?;
        range(line, Self::MAX_LINE, "line", s)?;
        range(device, Self::MAX_DEVICE, "device", s)?;
        Ok(Self::from_raw((area << 12) | (line << 8) | device))
    }

    /// The raw 16-bit value.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.raw
    }

    /// The area part (high nibble).
    #[must_use]
    pub const fn area(self) -> u16 {
        (self.raw >> 12) & Self::MAX_AREA
    }

    /// The line part (middle nibble).
    #[must_use]
    pub const fn line(self) -> u16 {
        (self.raw >> 8) & Self::MAX_LINE
    }

    /// The device part (low byte).
    #[must_use]
    pub const fn device(self) -> u16 {
        self.raw & Self::MAX_DEVICE
    }
}

impl fmt::Display for IndividualAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.area(), self.line(), self.device())
    }
}

/// A KNX logical group address — the destination an action targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupAddress {
    raw: u16,
    style: GroupAddressStyle,
}

impl GroupAddress {
    /// Highest valid main group (5 bits).
    pub const MAX_MAIN: u16 = 31;
    /// Highest valid middle group, 3-level only (3 bits).
    pub const MAX_MIDDLE: u16 = 7;
    /// Highest valid sub group in 3-level notation (8 bits).
    pub const MAX_SUB_THREE: u16 = 255;
    /// Highest valid sub group in 2-level notation (11 bits).
    pub const MAX_SUB_TWO: u16 = 2047;

    /// Build from the raw 16-bit value, defaulting to 3-level display style.
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        Self {
            raw,
            style: GroupAddressStyle::ThreeLevel,
        }
    }

    /// Set the notation this address is displayed in. Does not change the wire
    /// value.
    #[must_use]
    pub const fn with_style(mut self, style: GroupAddressStyle) -> Self {
        self.style = style;
        self
    }

    /// Decode from the canonical 2-byte big-endian wire representation.
    #[must_use]
    pub const fn from_bytes(raw: [u8; 2]) -> Self {
        Self::from_raw(u16::from_be_bytes(raw))
    }

    /// Encode to the canonical 2-byte big-endian wire representation.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 2] {
        self.raw.to_be_bytes()
    }

    /// Parse a group address in 3-level (`"1/2/3"`), 2-level (`"1/500"`), or
    /// free (`"12345"`) notation. The notation is inferred from the input and
    /// recorded so [`Display`](fmt::Display) round-trips it.
    ///
    /// # Errors
    /// Returns [`KnxError::AddressParse`] on a bad shape, a non-numeric part, or
    /// an out-of-range field.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        if !s.contains('/') {
            let n = s
                .parse::<u16>()
                .map_err(|_| KnxError::AddressParse(format!("not a number: {s:?}")))?;
            return Ok(Self::from_raw(n).with_style(GroupAddressStyle::Free));
        }
        let parts: Vec<&str> = s.split('/').collect();
        match parts.len() {
            2 => {
                let main = parse_part(parts[0], "main", s)?;
                let sub = parse_part(parts[1], "sub", s)?;
                range(main, Self::MAX_MAIN, "main", s)?;
                range(sub, Self::MAX_SUB_TWO, "sub", s)?;
                Ok(Self {
                    raw: (main << 11) | sub,
                    style: GroupAddressStyle::TwoLevel,
                })
            }
            3 => {
                let main = parse_part(parts[0], "main", s)?;
                let middle = parse_part(parts[1], "middle", s)?;
                let sub = parse_part(parts[2], "sub", s)?;
                range(main, Self::MAX_MAIN, "main", s)?;
                range(middle, Self::MAX_MIDDLE, "middle", s)?;
                range(sub, Self::MAX_SUB_THREE, "sub", s)?;
                Ok(Self {
                    raw: (main << 11) | (middle << 8) | sub,
                    style: GroupAddressStyle::ThreeLevel,
                })
            }
            _ => Err(KnxError::AddressParse(format!(
                "too many parts in group address {s:?}"
            ))),
        }
    }

    /// The raw 16-bit value.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.raw
    }

    /// The display notation.
    #[must_use]
    pub const fn style(self) -> GroupAddressStyle {
        self.style
    }

    /// The main group, or `None` in free notation.
    #[must_use]
    pub const fn main(self) -> Option<u16> {
        match self.style {
            GroupAddressStyle::Free => None,
            _ => Some((self.raw >> 11) & Self::MAX_MAIN),
        }
    }

    /// The middle group; only meaningful in 3-level notation.
    #[must_use]
    pub const fn middle(self) -> Option<u16> {
        match self.style {
            GroupAddressStyle::ThreeLevel => Some((self.raw >> 8) & Self::MAX_MIDDLE),
            _ => None,
        }
    }

    /// The sub group, masked for the current notation.
    #[must_use]
    pub const fn sub(self) -> u16 {
        match self.style {
            GroupAddressStyle::Free => self.raw,
            GroupAddressStyle::TwoLevel => self.raw & Self::MAX_SUB_TWO,
            GroupAddressStyle::ThreeLevel => self.raw & Self::MAX_SUB_THREE,
        }
    }
}

impl fmt::Display for GroupAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.style {
            GroupAddressStyle::Free => write!(f, "{}", self.raw),
            GroupAddressStyle::TwoLevel => {
                write!(f, "{}/{}", (self.raw >> 11) & Self::MAX_MAIN, self.sub())
            }
            GroupAddressStyle::ThreeLevel => write!(
                f,
                "{}/{}/{}",
                (self.raw >> 11) & Self::MAX_MAIN,
                (self.raw >> 8) & Self::MAX_MIDDLE,
                self.sub()
            ),
        }
    }
}

/// Parse one dotted/slashed numeric part with a descriptive error.
fn parse_part(part: &str, name: &str, full: &str) -> Result<u16> {
    part.trim()
        .parse::<u16>()
        .map_err(|_| KnxError::AddressParse(format!("{name} part is not a number in {full:?}")))
}

/// Reject a part that exceeds its field width.
fn range(value: u16, max: u16, name: &str, full: &str) -> Result<()> {
    if value > max {
        return Err(KnxError::AddressParse(format!(
            "{name} part {value} out of range (0..={max}) in {full:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn individual_parse_three_level() {
        let a = IndividualAddress::parse("1.1.5").expect("parse");
        assert_eq!(a.area(), 1);
        assert_eq!(a.line(), 1);
        assert_eq!(a.device(), 5);
        assert_eq!(a.raw(), (1 << 12) | (1 << 8) | 5);
        assert_eq!(a.to_string(), "1.1.5");
    }

    #[test]
    fn individual_roundtrip_string_raw_bytes() {
        let a = IndividualAddress::parse("15.15.255").expect("parse");
        assert_eq!(a.raw(), 0xFFFF);
        assert_eq!(a.to_bytes(), [0xFF, 0xFF]);
        assert_eq!(IndividualAddress::from_bytes(a.to_bytes()), a);
        assert_eq!(IndividualAddress::from_raw(a.raw()).to_string(), "15.15.255");
    }

    #[test]
    fn individual_bare_number() {
        let a = IndividualAddress::parse("4353").expect("parse"); // 0x1101 = 1.1.1
        assert_eq!(a.to_string(), "1.1.1");
    }

    #[test]
    fn individual_out_of_range() {
        assert!(IndividualAddress::parse("16.0.0").is_err());
        assert!(IndividualAddress::parse("0.16.0").is_err());
        assert!(IndividualAddress::parse("0.0.256").is_err());
    }

    #[test]
    fn individual_malformed() {
        assert!(IndividualAddress::parse("1.1").is_err());
        assert!(IndividualAddress::parse("1.x.3").is_err());
        assert!(IndividualAddress::parse("").is_err());
    }

    #[test]
    fn group_three_level() {
        let g = GroupAddress::parse("1/2/3").expect("parse");
        assert_eq!(g.main(), Some(1));
        assert_eq!(g.middle(), Some(2));
        assert_eq!(g.sub(), 3);
        assert_eq!(g.raw(), (1 << 11) | (2 << 8) | 3);
        assert_eq!(g.to_string(), "1/2/3");
        assert_eq!(g.style(), GroupAddressStyle::ThreeLevel);
    }

    #[test]
    fn group_two_level() {
        let g = GroupAddress::parse("1/500").expect("parse");
        assert_eq!(g.main(), Some(1));
        assert_eq!(g.middle(), None);
        assert_eq!(g.sub(), 500);
        assert_eq!(g.raw(), (1 << 11) | 500);
        assert_eq!(g.to_string(), "1/500");
        assert_eq!(g.style(), GroupAddressStyle::TwoLevel);
    }

    #[test]
    fn group_free() {
        let g = GroupAddress::parse("12345").expect("parse");
        assert_eq!(g.main(), None);
        assert_eq!(g.middle(), None);
        assert_eq!(g.sub(), 12345);
        assert_eq!(g.to_string(), "12345");
        assert_eq!(g.style(), GroupAddressStyle::Free);
    }

    #[test]
    fn group_roundtrip_bytes() {
        let g = GroupAddress::parse("4/2/16").expect("parse");
        let bytes = g.to_bytes();
        assert_eq!(GroupAddress::from_bytes(bytes).raw(), g.raw());
    }

    #[test]
    fn group_out_of_range() {
        assert!(GroupAddress::parse("32/0/0").is_err()); // main > 31
        assert!(GroupAddress::parse("0/8/0").is_err()); // middle > 7
        assert!(GroupAddress::parse("0/0/256").is_err()); // sub > 255
        assert!(GroupAddress::parse("0/2048").is_err()); // 2-level sub > 2047
    }

    #[test]
    fn group_malformed() {
        assert!(GroupAddress::parse("1/2/3/4").is_err());
        assert!(GroupAddress::parse("a/2/3").is_err());
    }

    #[test]
    fn group_style_override_changes_display_not_wire() {
        let raw = GroupAddress::parse("1/2/3").expect("parse").raw();
        let g = GroupAddress::from_raw(raw).with_style(GroupAddressStyle::TwoLevel);
        assert_eq!(g.raw(), raw); // wire value unchanged
        assert_eq!(g.to_string(), "1/515"); // (2<<8)|3 = 515 in 2-level
    }
}
