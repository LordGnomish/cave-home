// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §8.2 (CSA public PDF) only; Z2M source NOT consulted
//! IAS Zone cluster (0x0500) — ZCL §8.2.
//!
//! The Intruder Alarm System (IAS) Zone cluster is the one behind every
//! security sensor: motion (PIR), door/window contact, smoke, water-leak,
//! CO, glass-break. The persona gets a "Kapı açıldı" / "Duman algılandı"
//! card in the Portal; under the hood the device sends a Zone Status Change
//! Notification (§8.2.2.5.1) whose `ZoneStatus` bitmap carries the alarm.
//!
//! Phase 1 implements the attribute identifiers (§8.2.2.2), the `ZoneType`
//! table (§8.2.2.2.1), the `ZoneStatus` bitmap (§8.2.2.2.2), the two
//! server→client notifications (§8.2.2.5), and the three client→server
//! enrollment / test commands (§8.2.2.4).

use crate::error::{Result, ZigbeeError};

/// IAS Zone cluster identifier (ZCL §8.2.1).
pub const IAS_ZONE_CLUSTER_ID: u16 = 0x0500;

/// Received-command identifiers (client→server) — ZCL §8.2.2.4.
pub mod command_id {
    /// Zone Enroll Response (0x00).
    pub const ZONE_ENROLL_RESPONSE: u8 = 0x00;
    /// Initiate Normal Operation Mode (0x01).
    pub const INITIATE_NORMAL_OPERATION_MODE: u8 = 0x01;
    /// Initiate Test Mode (0x02).
    pub const INITIATE_TEST_MODE: u8 = 0x02;
}

/// Generated-command identifiers (server→client) — ZCL §8.2.2.5.
pub mod notification_id {
    /// Zone Status Change Notification (0x00).
    pub const ZONE_STATUS_CHANGE_NOTIFICATION: u8 = 0x00;
    /// Zone Enroll Request (0x01).
    pub const ZONE_ENROLL_REQUEST: u8 = 0x01;
}

/// Attribute identifiers — ZCL §8.2.2.2.
pub mod attribute_id {
    /// `ZoneState` (0x0000, enum8) — enrolled / not enrolled.
    pub const ZONE_STATE: u16 = 0x0000;
    /// `ZoneType` (0x0001, enum16).
    pub const ZONE_TYPE: u16 = 0x0001;
    /// `ZoneStatus` (0x0002, bitmap16).
    pub const ZONE_STATUS: u16 = 0x0002;
    /// `IAS_CIE_Address` (0x0010, IEEE address).
    pub const IAS_CIE_ADDRESS: u16 = 0x0010;
    /// `ZoneID` (0x0011, uint8).
    pub const ZONE_ID: u16 = 0x0011;
}

/// Zone type — ZCL §8.2.2.2.1 Table 8-4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZoneType {
    /// 0x0000 — Standard CIE (control & indicating equipment).
    StandardCie,
    /// 0x000d — Motion sensor (PIR).
    MotionSensor,
    /// 0x0015 — Contact switch (door / window).
    ContactSwitch,
    /// 0x0028 — Fire / smoke sensor.
    FireSensor,
    /// 0x002a — Water-leak sensor.
    WaterSensor,
    /// 0x002b — Carbon-monoxide sensor.
    CarbonMonoxide,
    /// 0x002c — Personal emergency device.
    PersonalEmergency,
    /// 0x002d — Vibration / movement sensor.
    VibrationMovement,
    /// 0x010f — Remote control.
    RemoteControl,
    /// 0x0115 — Key fob.
    KeyFob,
    /// 0x021d — Keypad.
    Keypad,
    /// 0x0225 — Standard warning device.
    StandardWarning,
    /// 0x0226 — Glass-break sensor.
    GlassBreak,
    /// 0x0229 — Security repeater.
    SecurityRepeater,
    /// 0xffff — Invalid zone type.
    Invalid,
    /// Any other / manufacturer-specific raw value.
    Other(u16),
}

impl ZoneType {
    /// Decode from the enum16 wire value (total — unknown maps to `Other`).
    #[must_use]
    pub const fn from_u16(v: u16) -> Self {
        match v {
            0x0000 => Self::StandardCie,
            0x000d => Self::MotionSensor,
            0x0015 => Self::ContactSwitch,
            0x0028 => Self::FireSensor,
            0x002a => Self::WaterSensor,
            0x002b => Self::CarbonMonoxide,
            0x002c => Self::PersonalEmergency,
            0x002d => Self::VibrationMovement,
            0x010f => Self::RemoteControl,
            0x0115 => Self::KeyFob,
            0x021d => Self::Keypad,
            0x0225 => Self::StandardWarning,
            0x0226 => Self::GlassBreak,
            0x0229 => Self::SecurityRepeater,
            0xffff => Self::Invalid,
            other => Self::Other(other),
        }
    }

    /// Encode to the enum16 wire value.
    #[must_use]
    pub const fn to_u16(self) -> u16 {
        match self {
            Self::StandardCie => 0x0000,
            Self::MotionSensor => 0x000d,
            Self::ContactSwitch => 0x0015,
            Self::FireSensor => 0x0028,
            Self::WaterSensor => 0x002a,
            Self::CarbonMonoxide => 0x002b,
            Self::PersonalEmergency => 0x002c,
            Self::VibrationMovement => 0x002d,
            Self::RemoteControl => 0x010f,
            Self::KeyFob => 0x0115,
            Self::Keypad => 0x021d,
            Self::StandardWarning => 0x0225,
            Self::GlassBreak => 0x0226,
            Self::SecurityRepeater => 0x0229,
            Self::Invalid => 0xffff,
            Self::Other(v) => v,
        }
    }
}

/// `ZoneStatus` bitmap16 — ZCL §8.2.2.2.2 Table 8-5.
///
/// Each bit is a latched flag the sensor reports. The persona only ever
/// sees the derived state ("Açık" / "Kapalı", "Pil zayıf"); the bitmap is
/// the on-wire representation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ZoneStatus(u16);

/// Named bit positions within [`ZoneStatus`] (ZCL §8.2.2.2.2).
pub mod zone_status_bit {
    /// Bit 0 — Alarm 1 (primary alarm: opened / motion / smoke).
    pub const ALARM1: u16 = 1 << 0;
    /// Bit 1 — Alarm 2 (secondary alarm).
    pub const ALARM2: u16 = 1 << 1;
    /// Bit 2 — Tamper.
    pub const TAMPER: u16 = 1 << 2;
    /// Bit 3 — Battery low.
    pub const BATTERY: u16 = 1 << 3;
    /// Bit 4 — Supervision reports.
    pub const SUPERVISION_REPORTS: u16 = 1 << 4;
    /// Bit 5 — Restore reports.
    pub const RESTORE_REPORTS: u16 = 1 << 5;
    /// Bit 6 — Trouble / failure.
    pub const TROUBLE: u16 = 1 << 6;
    /// Bit 7 — AC (mains) fault.
    pub const AC_MAINS: u16 = 1 << 7;
    /// Bit 8 — Test mode.
    pub const TEST: u16 = 1 << 8;
    /// Bit 9 — Battery defect.
    pub const BATTERY_DEFECT: u16 = 1 << 9;
}

impl ZoneStatus {
    /// Wrap a raw bitmap value.
    #[must_use]
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// The raw bitmap value.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Test a single bit.
    #[must_use]
    const fn get(self, mask: u16) -> bool {
        self.0 & mask != 0
    }

    /// Set or clear a single bit, returning the new value (builder style).
    #[must_use]
    const fn set(self, mask: u16, on: bool) -> Self {
        if on {
            Self(self.0 | mask)
        } else {
            Self(self.0 & !mask)
        }
    }

    /// Alarm 1 — primary alarm (opened / motion / smoke detected).
    #[must_use]
    pub const fn alarm1(self) -> bool {
        self.get(zone_status_bit::ALARM1)
    }
    /// Alarm 2 — secondary alarm.
    #[must_use]
    pub const fn alarm2(self) -> bool {
        self.get(zone_status_bit::ALARM2)
    }
    /// Tamper detected.
    #[must_use]
    pub const fn tamper(self) -> bool {
        self.get(zone_status_bit::TAMPER)
    }
    /// Battery low.
    #[must_use]
    pub const fn battery_low(self) -> bool {
        self.get(zone_status_bit::BATTERY)
    }
    /// Trouble / failure.
    #[must_use]
    pub const fn trouble(self) -> bool {
        self.get(zone_status_bit::TROUBLE)
    }
    /// Battery defective.
    #[must_use]
    pub const fn battery_defect(self) -> bool {
        self.get(zone_status_bit::BATTERY_DEFECT)
    }

    /// Builder: set Alarm 1.
    #[must_use]
    pub const fn with_alarm1(self, on: bool) -> Self {
        self.set(zone_status_bit::ALARM1, on)
    }
    /// Builder: set Tamper.
    #[must_use]
    pub const fn with_tamper(self, on: bool) -> Self {
        self.set(zone_status_bit::TAMPER, on)
    }
    /// Builder: set Battery low.
    #[must_use]
    pub const fn with_battery_low(self, on: bool) -> Self {
        self.set(zone_status_bit::BATTERY, on)
    }
}

/// Zone Status Change Notification (server→client) — ZCL §8.2.2.5.1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZoneStatusChangeNotification {
    /// The new zone status bitmap.
    pub zone_status: ZoneStatus,
    /// Extended status (reserved; transmit as 0).
    pub extended_status: u8,
    /// Zone ID assigned at enrollment.
    pub zone_id: u8,
    /// Delay (1/4 s units) since the change occurred.
    pub delay: u16,
}

impl ZoneStatusChangeNotification {
    /// Parse from the command payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] if fewer than 6 bytes.
    pub fn parse(payload: &[u8]) -> Result<Self> {
        require(payload, 6)?;
        Ok(Self {
            zone_status: ZoneStatus::from_bits(u16::from_le_bytes([payload[0], payload[1]])),
            extended_status: payload[2],
            zone_id: payload[3],
            delay: u16::from_le_bytes([payload[4], payload[5]]),
        })
    }

    /// Encode to the command payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6);
        out.extend_from_slice(&self.zone_status.bits().to_le_bytes());
        out.push(self.extended_status);
        out.push(self.zone_id);
        out.extend_from_slice(&self.delay.to_le_bytes());
        out
    }
}

/// Zone Enroll Request (server→client) — ZCL §8.2.2.5.2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZoneEnrollRequest {
    /// The device's zone type.
    pub zone_type: ZoneType,
    /// Manufacturer code.
    pub manufacturer_code: u16,
}

impl ZoneEnrollRequest {
    /// Parse from the command payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] if fewer than 4 bytes.
    pub fn parse(payload: &[u8]) -> Result<Self> {
        require(payload, 4)?;
        Ok(Self {
            zone_type: ZoneType::from_u16(u16::from_le_bytes([payload[0], payload[1]])),
            manufacturer_code: u16::from_le_bytes([payload[2], payload[3]]),
        })
    }

    /// Encode to the command payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4);
        out.extend_from_slice(&self.zone_type.to_u16().to_le_bytes());
        out.extend_from_slice(&self.manufacturer_code.to_le_bytes());
        out
    }
}

/// Enroll response code — ZCL §8.2.2.4.1 Table 8-8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnrollResponseCode {
    /// 0x00 — Success.
    Success,
    /// 0x01 — Not supported.
    NotSupported,
    /// 0x02 — No enroll permit.
    NoEnrollPermit,
    /// 0x03 — Too many zones.
    TooManyZones,
}

impl EnrollResponseCode {
    /// Decode from the wire value.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for a reserved value.
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x00 => Ok(Self::Success),
            0x01 => Ok(Self::NotSupported),
            0x02 => Ok(Self::NoEnrollPermit),
            0x03 => Ok(Self::TooManyZones),
            other => Err(ZigbeeError::Zcl(format!(
                "reserved enroll response code 0x{other:02x}"
            ))),
        }
    }

    /// Encode to the wire value.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Success => 0x00,
            Self::NotSupported => 0x01,
            Self::NoEnrollPermit => 0x02,
            Self::TooManyZones => 0x03,
        }
    }
}

/// A decoded IAS Zone received command (client→server) — ZCL §8.2.2.4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IasZoneCommand {
    /// Zone Enroll Response (0x00) — coordinator answers an enroll request.
    ZoneEnrollResponse {
        /// Enroll outcome.
        code: EnrollResponseCode,
        /// Zone ID assigned to the device.
        zone_id: u8,
    },
    /// Initiate Normal Operation Mode (0x01).
    InitiateNormalOperationMode,
    /// Initiate Test Mode (0x02).
    InitiateTestMode {
        /// Test-mode duration in seconds.
        test_mode_duration: u8,
        /// Current zone sensitivity level.
        current_zone_sensitivity_level: u8,
    },
}

impl IasZoneCommand {
    /// Parse a received command from its id + payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] for short payloads, [`ZigbeeError::Zcl`]
    /// for unknown command ids or reserved response codes.
    pub fn parse(command_id: u8, payload: &[u8]) -> Result<Self> {
        match command_id {
            command_id::ZONE_ENROLL_RESPONSE => {
                require(payload, 2)?;
                Ok(Self::ZoneEnrollResponse {
                    code: EnrollResponseCode::from_u8(payload[0])?,
                    zone_id: payload[1],
                })
            }
            command_id::INITIATE_NORMAL_OPERATION_MODE => Ok(Self::InitiateNormalOperationMode),
            command_id::INITIATE_TEST_MODE => {
                require(payload, 2)?;
                Ok(Self::InitiateTestMode {
                    test_mode_duration: payload[0],
                    current_zone_sensitivity_level: payload[1],
                })
            }
            other => Err(ZigbeeError::Zcl(format!(
                "unknown IAS Zone command 0x{other:02x}"
            ))),
        }
    }

    /// The command identifier for this command.
    #[must_use]
    pub const fn command_id(&self) -> u8 {
        match self {
            Self::ZoneEnrollResponse { .. } => command_id::ZONE_ENROLL_RESPONSE,
            Self::InitiateNormalOperationMode => command_id::INITIATE_NORMAL_OPERATION_MODE,
            Self::InitiateTestMode { .. } => command_id::INITIATE_TEST_MODE,
        }
    }

    /// Encode the command-specific payload.
    #[must_use]
    pub fn encode_payload(&self) -> Vec<u8> {
        match self {
            Self::ZoneEnrollResponse { code, zone_id } => vec![code.to_u8(), *zone_id],
            Self::InitiateNormalOperationMode => Vec::new(),
            Self::InitiateTestMode {
                test_mode_duration,
                current_zone_sensitivity_level,
            } => vec![*test_mode_duration, *current_zone_sensitivity_level],
        }
    }
}

/// Require at least `n` payload bytes.
const fn require(payload: &[u8], n: usize) -> Result<()> {
    if payload.len() < n {
        Err(ZigbeeError::Truncated {
            need: n,
            have: payload.len(),
        })
    } else {
        Ok(())
    }
}
