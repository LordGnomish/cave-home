// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §3.5 (CSA public PDF) only; Z2M source NOT consulted
//! Occupancy Sensing cluster (0x0406) — ZCL §3.5.
//!
//! Presence sensors. The persona sees "Salonda hareket var" (motion in the
//! living room) and automations like "turn the hall light on when someone
//! walks in". The cluster is attribute-only (no client→server commands): the
//! device reports its `Occupancy` bitmap via the Foundation Report Attributes
//! command (see [`crate::attribute_reporting`]).
//!
//! Phase 1 implements the attribute identifiers (§3.5.2.2), the `Occupancy`
//! bitmap (§3.5.2.2.1), the `OccupancySensorType` enum (§3.5.2.2.2) and its
//! bitmap form (§3.5.2.2.3), plus an in-memory state holder.

use crate::error::{Result, ZigbeeError};

/// Occupancy Sensing cluster identifier (ZCL §3.5.1).
pub const OCCUPANCY_SENSING_CLUSTER_ID: u16 = 0x0406;

/// Attribute identifiers — ZCL §3.5.2.2.
pub mod attribute_id {
    /// Occupancy (0x0000, bitmap8).
    pub const OCCUPANCY: u16 = 0x0000;
    /// `OccupancySensorType` (0x0001, enum8).
    pub const OCCUPANCY_SENSOR_TYPE: u16 = 0x0001;
    /// `OccupancySensorTypeBitmap` (0x0002, bitmap8).
    pub const OCCUPANCY_SENSOR_TYPE_BITMAP: u16 = 0x0002;
    /// `PIROccupiedToUnoccupiedDelay` (0x0010, uint16).
    pub const PIR_OCCUPIED_TO_UNOCCUPIED_DELAY: u16 = 0x0010;
    /// `PIRUnoccupiedToOccupiedDelay` (0x0011, uint16).
    pub const PIR_UNOCCUPIED_TO_OCCUPIED_DELAY: u16 = 0x0011;
    /// `PIRUnoccupiedToOccupiedThreshold` (0x0012, uint8).
    pub const PIR_UNOCCUPIED_TO_OCCUPIED_THRESHOLD: u16 = 0x0012;
    /// `UltrasonicOccupiedToUnoccupiedDelay` (0x0020, uint16).
    pub const ULTRASONIC_OCCUPIED_TO_UNOCCUPIED_DELAY: u16 = 0x0020;
    /// `UltrasonicUnoccupiedToOccupiedDelay` (0x0021, uint16).
    pub const ULTRASONIC_UNOCCUPIED_TO_OCCUPIED_DELAY: u16 = 0x0021;
    /// `UltrasonicUnoccupiedToOccupiedThreshold` (0x0022, uint8).
    pub const ULTRASONIC_UNOCCUPIED_TO_OCCUPIED_THRESHOLD: u16 = 0x0022;
    /// `PhysicalContactOccupiedToUnoccupiedDelay` (0x0030, uint16).
    pub const PHYSICAL_CONTACT_OCCUPIED_TO_UNOCCUPIED_DELAY: u16 = 0x0030;
    /// `PhysicalContactUnoccupiedToOccupiedDelay` (0x0031, uint16).
    pub const PHYSICAL_CONTACT_UNOCCUPIED_TO_OCCUPIED_DELAY: u16 = 0x0031;
    /// `PhysicalContactUnoccupiedToOccupiedThreshold` (0x0032, uint8).
    pub const PHYSICAL_CONTACT_UNOCCUPIED_TO_OCCUPIED_THRESHOLD: u16 = 0x0032;
}

/// Occupancy bitmap8 — ZCL §3.5.2.2.1. Only bit 0 (occupied) is defined.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Occupancy(u8);

impl Occupancy {
    /// Wrap a raw bitmap value.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// Construct from a logical occupied flag.
    #[must_use]
    pub const fn occupied_state(occupied: bool) -> Self {
        Self(if occupied { 0x01 } else { 0x00 })
    }

    /// The raw bitmap value.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// `true` if bit 0 (occupied) is set.
    #[must_use]
    pub const fn occupied(self) -> bool {
        self.0 & 0x01 != 0
    }
}

/// `OccupancySensorType` enum8 — ZCL §3.5.2.2.2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OccupancySensorType {
    /// 0x00 — Passive infrared.
    Pir,
    /// 0x01 — Ultrasonic.
    Ultrasonic,
    /// 0x02 — PIR and ultrasonic.
    PirAndUltrasonic,
    /// 0x03 — Physical contact.
    PhysicalContact,
}

impl OccupancySensorType {
    /// Decode from the wire value.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for a reserved value.
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x00 => Ok(Self::Pir),
            0x01 => Ok(Self::Ultrasonic),
            0x02 => Ok(Self::PirAndUltrasonic),
            0x03 => Ok(Self::PhysicalContact),
            other => Err(ZigbeeError::Zcl(format!(
                "reserved occupancy sensor type 0x{other:02x}"
            ))),
        }
    }

    /// Encode to the wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Pir => 0x00,
            Self::Ultrasonic => 0x01,
            Self::PirAndUltrasonic => 0x02,
            Self::PhysicalContact => 0x03,
        }
    }
}

/// `OccupancySensorTypeBitmap` bitmap8 — ZCL §3.5.2.2.3.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SensorTypeBitmap(u8);

impl SensorTypeBitmap {
    /// Wrap a raw bitmap value.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// The raw bitmap value.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Bit 0 — PIR.
    #[must_use]
    pub const fn pir(self) -> bool {
        self.0 & (1 << 0) != 0
    }
    /// Bit 1 — Ultrasonic.
    #[must_use]
    pub const fn ultrasonic(self) -> bool {
        self.0 & (1 << 1) != 0
    }
    /// Bit 2 — Physical contact.
    #[must_use]
    pub const fn physical_contact(self) -> bool {
        self.0 & (1 << 2) != 0
    }

    /// Builder: set the PIR bit.
    #[must_use]
    pub const fn with_pir(self, on: bool) -> Self {
        Self::set(self, 1 << 0, on)
    }
    /// Builder: set the ultrasonic bit.
    #[must_use]
    pub const fn with_ultrasonic(self, on: bool) -> Self {
        Self::set(self, 1 << 1, on)
    }
    /// Builder: set the physical-contact bit.
    #[must_use]
    pub const fn with_physical_contact(self, on: bool) -> Self {
        Self::set(self, 1 << 2, on)
    }

    const fn set(self, mask: u8, on: bool) -> Self {
        if on {
            Self(self.0 | mask)
        } else {
            Self(self.0 & !mask)
        }
    }
}

/// In-memory Occupancy Sensing state for one endpoint (§3.5.2.2 attributes).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccupancyState {
    /// Occupancy attribute (0x0000).
    pub occupancy: Occupancy,
    /// `OccupancySensorType` attribute (0x0001).
    pub sensor_type: OccupancySensorType,
}

impl Default for OccupancyState {
    fn default() -> Self {
        Self {
            occupancy: Occupancy::occupied_state(false),
            sensor_type: OccupancySensorType::Pir,
        }
    }
}

impl OccupancyState {
    /// Fresh state — unoccupied PIR sensor.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the Occupancy attribute from a logical occupied flag.
    pub const fn set_occupied(&mut self, occupied: bool) {
        self.occupancy = Occupancy::occupied_state(occupied);
    }
}
