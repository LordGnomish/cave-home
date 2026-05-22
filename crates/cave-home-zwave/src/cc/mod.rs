// SPDX-License-Identifier: Apache-2.0
//! Z-Wave Command Classes — Phase 1 set.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/cc/src/cc/
//!
//! Each Phase 1 CC has its own file. The wire format is the same for all:
//!
//! ```text
//!   CC_ID | CMD | <payload …>
//! ```
//!
//! where `CC_ID` is a single byte from [`CommandClassId`] and `CMD` is the
//! per-class command discriminator.

pub mod basic;
pub mod battery;
pub mod binary_switch;
pub mod configuration;
pub mod multilevel_sensor;
pub mod multilevel_switch;
pub mod notification;
// Phase 1b CCs (declared in CommandClassId enum for wire-byte parity, but
// the per-class encoder/decoder modules land in the next sweep):
// pub mod manufacturer_specific;
// pub mod version;
// pub mod wake_up;

/// Command Class identifier byte (subset).
///
/// # Upstream: `core/src/definitions/CommandClasses.ts::CommandClasses`
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum CommandClassId {
    /// `Basic` (0x20) — coarse-grained on/off/dim.
    Basic = 0x20,
    /// `Multilevel Switch` (0x26) — dimmers, blinds.
    MultilevelSwitch = 0x26,
    /// `Binary Switch` (0x25) — relays, smart plugs.
    BinarySwitch = 0x25,
    /// `Multilevel Sensor` (0x31) — temperature / humidity / illuminance.
    MultilevelSensor = 0x31,
    /// `Configuration` (0x70) — vendor-defined parameters.
    Configuration = 0x70,
    /// `Notification` (0x71) — motion / door / smoke / glass-break.
    Notification = 0x71,
    /// `Manufacturer Specific` (0x72).
    ManufacturerSpecific = 0x72,
    /// `Battery` (0x80).
    Battery = 0x80,
    /// `Wake Up` (0x84).
    WakeUp = 0x84,
    /// `Version` (0x86).
    Version = 0x86,
}

impl CommandClassId {
    /// Decode from the wire byte. Returns `None` for unsupported / unknown
    /// CC IDs in Phase 1.
    #[must_use]
    pub const fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x20 => Some(Self::Basic),
            0x25 => Some(Self::BinarySwitch),
            0x26 => Some(Self::MultilevelSwitch),
            0x31 => Some(Self::MultilevelSensor),
            0x70 => Some(Self::Configuration),
            0x71 => Some(Self::Notification),
            0x72 => Some(Self::ManufacturerSpecific),
            0x80 => Some(Self::Battery),
            0x84 => Some(Self::WakeUp),
            0x86 => Some(Self::Version),
            _ => None,
        }
    }

    /// Encode to the wire byte.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wire bytes are normative — they must match the Z-Wave 2018 catalogue.
    #[test]
    fn command_class_ids_match_upstream() {
        assert_eq!(CommandClassId::Basic.as_u8(), 0x20);
        assert_eq!(CommandClassId::BinarySwitch.as_u8(), 0x25);
        assert_eq!(CommandClassId::MultilevelSwitch.as_u8(), 0x26);
        assert_eq!(CommandClassId::MultilevelSensor.as_u8(), 0x31);
        assert_eq!(CommandClassId::Configuration.as_u8(), 0x70);
        assert_eq!(CommandClassId::Notification.as_u8(), 0x71);
        assert_eq!(CommandClassId::ManufacturerSpecific.as_u8(), 0x72);
        assert_eq!(CommandClassId::Battery.as_u8(), 0x80);
        assert_eq!(CommandClassId::WakeUp.as_u8(), 0x84);
        assert_eq!(CommandClassId::Version.as_u8(), 0x86);
    }

    #[test]
    fn round_trip() {
        for c in [
            CommandClassId::Basic,
            CommandClassId::BinarySwitch,
            CommandClassId::MultilevelSwitch,
            CommandClassId::MultilevelSensor,
            CommandClassId::Configuration,
            CommandClassId::Notification,
            CommandClassId::ManufacturerSpecific,
            CommandClassId::Battery,
            CommandClassId::WakeUp,
            CommandClassId::Version,
        ] {
            assert_eq!(CommandClassId::from_u8(c.as_u8()), Some(c));
        }
        assert_eq!(CommandClassId::from_u8(0xff), None);
    }
}
