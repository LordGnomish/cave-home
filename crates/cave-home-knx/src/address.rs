// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/telegram/address.py
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! Module for serialization/deserialization and handling of KNX addresses.
//!
//! The module can handle:
//! * individual addresses of devices.
//! * (logical) group addresses.
//! * xknx internal group addresses.
//!
//! The module supports all different writings of group addresses:
//! * 3rd level: "1/2/3"
//! * 2nd level: "1/2"
//! * Free format: "123"

use core::fmt;

use crate::error::{KnxError, Result};

/// Possible string formats of a [`GroupAddress`].
///
/// KNX knows three types of group addresses:
/// * `Free`, an integer or hex representation
/// * `Short`, a representation like `"1/123"`, without middle groups
/// * `Long`, a representation like `"1/2/34"`, with middle groups
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GroupAddressType {
    Free,
    Short,
    /// Default — KNX 3-level notation, matches xknx upstream.
    #[default]
    Long,
}

/// KNX Individual Address (a.b.c, area.main.line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndividualAddress {
    raw: u16,
}

impl IndividualAddress {
    pub const MAX_AREA: u16 = 15;
    pub const MAX_MAIN: u16 = 15;
    pub const MAX_LINE: u16 = 255;

    /// Build from a raw `u16` value (cannot fail; full `u16` range valid).
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        Self { raw }
    }

    /// Parse from the canonical KNX/IP 2-byte representation.
    pub fn from_knx(raw: [u8; 2]) -> Self {
        Self::from_raw(u16::from_be_bytes(raw))
    }

    /// Serialize to the canonical KNX/IP 2-byte representation.
    #[must_use]
    pub const fn to_knx(self) -> [u8; 2] {
        self.raw.to_be_bytes()
    }

    /// Parse a string of the form "1.2.3".
    pub fn parse(s: &str) -> Result<Self> {
        if let Ok(n) = s.parse::<u16>() {
            return Ok(Self::from_raw(n));
        }
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(KnxError::AddressParse(format!("invalid format: {s}")));
        }
        let area: u16 = parts[0]
            .parse()
            .map_err(|_| KnxError::AddressParse(format!("area not numeric: {s}")))?;
        let main: u16 = parts[1]
            .parse()
            .map_err(|_| KnxError::AddressParse(format!("main not numeric: {s}")))?;
        let line: u16 = parts[2]
            .parse()
            .map_err(|_| KnxError::AddressParse(format!("line not numeric: {s}")))?;
        if area > Self::MAX_AREA {
            return Err(KnxError::AddressParse(format!(
                "Area part out of range (0..{})",
                Self::MAX_AREA
            )));
        }
        if main > Self::MAX_MAIN {
            return Err(KnxError::AddressParse(format!(
                "Line part out of range (0..{})",
                Self::MAX_MAIN
            )));
        }
        if line > Self::MAX_LINE {
            return Err(KnxError::AddressParse(format!(
                "Device part out of range (0..{})",
                Self::MAX_LINE
            )));
        }
        Ok(Self::from_raw((area << 12) | (main << 8) | line))
    }

    #[must_use]
    pub const fn raw(self) -> u16 {
        self.raw
    }

    #[must_use]
    pub const fn area(self) -> u16 {
        (self.raw >> 12) & Self::MAX_AREA
    }

    #[must_use]
    pub const fn main(self) -> u16 {
        (self.raw >> 8) & Self::MAX_MAIN
    }

    #[must_use]
    pub const fn line(self) -> u16 {
        self.raw & Self::MAX_LINE
    }

    /// Return `true` if this address is a valid device address (line != 0).
    #[must_use]
    pub const fn is_device(self) -> bool {
        self.line() != 0
    }

    /// Return `true` if this address is a valid line address (line == 0).
    #[must_use]
    pub const fn is_line(self) -> bool {
        !self.is_device()
    }
}

impl fmt::Display for IndividualAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.area(), self.main(), self.line())
    }
}

/// KNX (logical) Group Address. Default format is 3-level ("1/2/3").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupAddress {
    raw: u16,
    address_format: GroupAddressType,
}

impl GroupAddress {
    pub const MAX_MAIN: u16 = 31;
    pub const MAX_MIDDLE: u16 = 7;
    pub const MAX_SUB_LONG: u16 = 255;
    pub const MAX_SUB_SHORT: u16 = 2047;

    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        Self {
            raw,
            address_format: GroupAddressType::Long,
        }
    }

    #[must_use]
    pub const fn with_format(mut self, fmt: GroupAddressType) -> Self {
        self.address_format = fmt;
        self
    }

    pub fn from_knx(raw: [u8; 2]) -> Self {
        Self::from_raw(u16::from_be_bytes(raw))
    }

    #[must_use]
    pub const fn to_knx(self) -> [u8; 2] {
        self.raw.to_be_bytes()
    }

    /// Parse a string in 2-level, 3-level, or free format.
    pub fn parse(s: &str) -> Result<Self> {
        if let Ok(n) = s.parse::<u16>() {
            return Ok(Self::from_raw(n));
        }
        let parts: Vec<&str> = s.split('/').collect();
        match parts.len() {
            2 => {
                let main: u16 = parts[0]
                    .parse()
                    .map_err(|_| KnxError::AddressParse(format!("main not numeric: {s}")))?;
                let sub: u16 = parts[1]
                    .parse()
                    .map_err(|_| KnxError::AddressParse(format!("sub not numeric: {s}")))?;
                if main > Self::MAX_MAIN {
                    return Err(KnxError::AddressParse(format!(
                        "Main group out of range (0..{})",
                        Self::MAX_MAIN
                    )));
                }
                if sub > Self::MAX_SUB_SHORT {
                    return Err(KnxError::AddressParse(format!(
                        "Sub group out of range (0..{})",
                        Self::MAX_SUB_SHORT
                    )));
                }
                Ok(Self::from_raw((main << 11) | sub))
            }
            3 => {
                let main: u16 = parts[0]
                    .parse()
                    .map_err(|_| KnxError::AddressParse(format!("main not numeric: {s}")))?;
                let middle: u16 = parts[1]
                    .parse()
                    .map_err(|_| KnxError::AddressParse(format!("middle not numeric: {s}")))?;
                let sub: u16 = parts[2]
                    .parse()
                    .map_err(|_| KnxError::AddressParse(format!("sub not numeric: {s}")))?;
                if main > Self::MAX_MAIN {
                    return Err(KnxError::AddressParse(format!(
                        "Main group out of range (0..{})",
                        Self::MAX_MAIN
                    )));
                }
                if middle > Self::MAX_MIDDLE {
                    return Err(KnxError::AddressParse(format!(
                        "Middle group out of range (0..{})",
                        Self::MAX_MIDDLE
                    )));
                }
                if sub > Self::MAX_SUB_LONG {
                    return Err(KnxError::AddressParse(format!(
                        "Sub group out of range (0..{})",
                        Self::MAX_SUB_LONG
                    )));
                }
                Ok(Self::from_raw((main << 11) | (middle << 8) | sub))
            }
            _ => Err(KnxError::AddressParse(format!("invalid format: {s}"))),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u16 {
        self.raw
    }

    /// Main group part. `None` if format is `Free`.
    #[must_use]
    pub const fn main(self) -> Option<u16> {
        match self.address_format {
            GroupAddressType::Free => None,
            _ => Some((self.raw >> 11) & Self::MAX_MAIN),
        }
    }

    /// Middle group part. Only meaningful for `Long`.
    #[must_use]
    pub const fn middle(self) -> Option<u16> {
        match self.address_format {
            GroupAddressType::Long => Some((self.raw >> 8) & Self::MAX_MIDDLE),
            _ => None,
        }
    }

    /// Sub group part — depends on chosen format.
    #[must_use]
    pub const fn sub(self) -> u16 {
        match self.address_format {
            GroupAddressType::Short => self.raw & Self::MAX_SUB_SHORT,
            GroupAddressType::Long => self.raw & Self::MAX_SUB_LONG,
            GroupAddressType::Free => self.raw,
        }
    }
}

impl fmt::Display for GroupAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.address_format {
            GroupAddressType::Long => write!(
                f,
                "{}/{}/{}",
                self.main().unwrap_or(0),
                self.middle().unwrap_or(0),
                self.sub()
            ),
            GroupAddressType::Short => {
                write!(f, "{}/{}", self.main().unwrap_or(0), self.sub())
            }
            GroupAddressType::Free => write!(f, "{}", self.sub()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn individual_address_parse_3_level() {
        let a = IndividualAddress::parse("1.2.3").expect("parse");
        assert_eq!(a.area(), 1);
        assert_eq!(a.main(), 2);
        assert_eq!(a.line(), 3);
        assert_eq!(a.raw(), (1 << 12) | (2 << 8) | 3);
        assert_eq!(a.to_string(), "1.2.3");
        assert!(a.is_device());
    }

    #[test]
    fn individual_address_line_address_no_device() {
        let a = IndividualAddress::parse("1.2.0").expect("parse");
        assert!(a.is_line());
        assert!(!a.is_device());
    }

    #[test]
    fn individual_address_out_of_range() {
        assert!(IndividualAddress::parse("16.0.0").is_err());
        assert!(IndividualAddress::parse("0.16.0").is_err());
        assert!(IndividualAddress::parse("0.0.256").is_err());
    }

    #[test]
    fn group_address_long_format() {
        let g = GroupAddress::parse("1/2/3").expect("parse");
        assert_eq!(g.main(), Some(1));
        assert_eq!(g.middle(), Some(2));
        assert_eq!(g.sub(), 3);
        assert_eq!(g.raw(), (1 << 11) | (2 << 8) | 3);
        assert_eq!(g.to_string(), "1/2/3");
    }

    #[test]
    fn group_address_short_format() {
        let g = GroupAddress::parse("1/123")
            .expect("parse")
            .with_format(GroupAddressType::Short);
        assert_eq!(g.main(), Some(1));
        assert_eq!(g.middle(), None);
        assert_eq!(g.sub(), 123);
        assert_eq!(g.to_string(), "1/123");
    }

    #[test]
    fn group_address_free_format() {
        let g = GroupAddress::parse("123")
            .expect("parse")
            .with_format(GroupAddressType::Free);
        assert_eq!(g.main(), None);
        assert_eq!(g.sub(), 123);
        assert_eq!(g.to_string(), "123");
    }

    #[test]
    fn group_address_out_of_range() {
        assert!(GroupAddress::parse("32/0/0").is_err());
        assert!(GroupAddress::parse("0/8/0").is_err());
        assert!(GroupAddress::parse("0/0/256").is_err());
    }

    #[test]
    fn knx_roundtrip_group_address() {
        let g = GroupAddress::parse("4/2/16").expect("parse");
        let bytes = g.to_knx();
        let g2 = GroupAddress::from_knx(bytes);
        assert_eq!(g.raw(), g2.raw());
    }

    #[test]
    fn knx_roundtrip_individual_address() {
        let a = IndividualAddress::parse("1.1.5").expect("parse");
        let bytes = a.to_knx();
        let a2 = IndividualAddress::from_knx(bytes);
        assert_eq!(a.raw(), a2.raw());
    }
}
