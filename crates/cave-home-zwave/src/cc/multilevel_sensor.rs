// SPDX-License-Identifier: Apache-2.0
//! `SensorMultilevelCC` — Get / Report.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/cc/src/cc/MultilevelSensorCC.ts
//!
//! Phase 1 covers the three most common scales the headline persona's home
//! actually surfaces: temperature, humidity, illuminance. (Battery sensors
//! use `BatteryCC` instead.)

use bytes::{BufMut, Bytes, BytesMut};

use super::CommandClassId;
use crate::error::{ZwaveError, ZwaveResult};

/// Command discriminator.
///
/// # Upstream: `_Types.ts::MultilevelSensorCommand`
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MultilevelSensorCommand {
    /// `Get`.
    Get = 0x04,
    /// `Report`.
    Report = 0x05,
}

/// Sensor type byte — Phase 1 subset.
///
/// # Upstream: `MultilevelSensorCC.ts` (Multilevel Sensor Type registry).
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SensorType {
    /// 0x01 — Air temperature.
    Temperature = 0x01,
    /// 0x03 — Illuminance.
    Illuminance = 0x03,
    /// 0x05 — Relative humidity.
    Humidity = 0x05,
}

impl SensorType {
    /// Decode from the wire byte. Returns `None` for sensor types Phase 1
    /// doesn't recognise yet.
    #[must_use]
    pub const fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Temperature),
            0x03 => Some(Self::Illuminance),
            0x05 => Some(Self::Humidity),
            _ => None,
        }
    }
}

/// Decoded sensor scale byte.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SensorScale {
    /// 0 for temperature = Celsius.
    Celsius,
    /// 1 for temperature = Fahrenheit.
    Fahrenheit,
    /// 0 for humidity = % RH.
    Percent,
    /// 1 for humidity = absolute g/m³.
    GramsPerCubicMetre,
    /// 0 for illuminance = % of max.
    PercentMax,
    /// 1 for illuminance = lux.
    Lux,
    /// Anything else — preserve the raw byte.
    Other(u8),
}

impl SensorScale {
    /// Decode using the sensor type's scale namespace.
    #[must_use]
    pub fn for_type(t: SensorType, b: u8) -> Self {
        match (t, b) {
            (SensorType::Temperature, 0) => Self::Celsius,
            (SensorType::Temperature, 1) => Self::Fahrenheit,
            (SensorType::Humidity, 0) => Self::Percent,
            (SensorType::Humidity, 1) => Self::GramsPerCubicMetre,
            (SensorType::Illuminance, 0) => Self::PercentMax,
            (SensorType::Illuminance, 1) => Self::Lux,
            (_, b) => Self::Other(b),
        }
    }
}

/// Multilevel Sensor CC payloads.
#[derive(Clone, Debug, PartialEq)]
pub enum MultilevelSensorCc {
    /// `Get` for a specific sensor type + scale (V5+) or empty (V1).
    Get {
        /// Sensor type byte (V5+).
        sensor_type: Option<u8>,
        /// Scale (V5+) — encoded into bits 3..4 of the third byte.
        scale: Option<u8>,
    },
    /// `Report`. The value is parsed per the precision/scale header.
    Report {
        /// Raw sensor type byte.
        sensor_type: u8,
        /// Raw scale byte.
        scale: u8,
        /// Precision (number of fractional digits) packed in bits 5..7 of
        /// the precision/scale/size byte.
        precision: u8,
        /// Floating-point value reconstructed from the size-bytes-of-int
        /// scaled by `10^-precision`.
        value: f64,
    },
}

impl MultilevelSensorCc {
    /// Encode to `CC_ID | CMD | payload`.
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u8(CommandClassId::MultilevelSensor.as_u8());
        match self {
            Self::Get { sensor_type, scale } => {
                buf.put_u8(MultilevelSensorCommand::Get as u8);
                if let Some(t) = sensor_type {
                    buf.put_u8(*t);
                    let scale_byte = (scale.unwrap_or(0) & 0b0000_0011) << 3;
                    buf.put_u8(scale_byte);
                }
            }
            Self::Report {
                sensor_type,
                scale,
                precision,
                value,
            } => {
                buf.put_u8(MultilevelSensorCommand::Report as u8);
                buf.put_u8(*sensor_type);
                // Pack precision (3 bits) | scale (2 bits) | size (3 bits).
                // Phase 1 always emits 4-byte values.
                let size: u8 = 4;
                let header = ((*precision & 0b0000_0111) << 5)
                    | ((*scale & 0b0000_0011) << 3)
                    | (size & 0b0000_0111);
                buf.put_u8(header);
                let raw = (value * f64::from(10i32.pow(u32::from(*precision)))) as i32;
                buf.put_i32(raw);
            }
        }
        buf.freeze()
    }

    /// Decode.
    ///
    /// # Errors
    /// Returns [`ZwaveError::PacketFormat`] for invalid sizes / unknown commands.
    pub fn decode(data: &[u8]) -> ZwaveResult<Self> {
        if data.len() < 2 {
            return Err(ZwaveError::PacketFormat(
                "MultilevelSensorCC: payload shorter than 2 bytes".into(),
            ));
        }
        if data[0] != CommandClassId::MultilevelSensor.as_u8() {
            return Err(ZwaveError::PacketFormat(format!(
                "MultilevelSensorCC: leading byte 0x{:02x} != 0x31",
                data[0]
            )));
        }
        match data[1] {
            0x04 => {
                let sensor_type = data.get(2).copied();
                let scale = data.get(3).map(|s| (s >> 3) & 0b0000_0011);
                Ok(Self::Get { sensor_type, scale })
            }
            0x05 => {
                if data.len() < 4 {
                    return Err(ZwaveError::PacketFormat(
                        "MultilevelSensorCCReport: missing header".into(),
                    ));
                }
                let sensor_type = data[2];
                let header = data[3];
                let precision = (header >> 5) & 0b0000_0111;
                let scale = (header >> 3) & 0b0000_0011;
                let size = (header & 0b0000_0111) as usize;
                if size == 0 || ![1usize, 2, 4].contains(&size) {
                    return Err(ZwaveError::PacketFormat(format!(
                        "MultilevelSensorCCReport: bad size {size}"
                    )));
                }
                if data.len() < 4 + size {
                    return Err(ZwaveError::PacketFormat(
                        "MultilevelSensorCCReport: value truncated".into(),
                    ));
                }
                // Sign-extend a 1/2/4-byte big-endian integer to i32.
                let mut raw: i32 = 0;
                for (i, b) in data[4..4 + size].iter().enumerate() {
                    if i == 0 {
                        raw = i32::from(*b as i8);
                    } else {
                        raw = (raw << 8) | i32::from(*b);
                    }
                }
                let value = f64::from(raw) / f64::from(10i32.pow(u32::from(precision)));
                Ok(Self::Report {
                    sensor_type,
                    scale,
                    precision,
                    value,
                })
            }
            other => Err(ZwaveError::PacketFormat(format!(
                "MultilevelSensorCC: unknown command 0x{other:02x}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_report_round_trip_celsius() {
        let cmd = MultilevelSensorCc::Report {
            sensor_type: SensorType::Temperature as u8,
            scale: 0, // Celsius
            precision: 2,
            value: 21.50,
        };
        let bytes = cmd.encode();
        let back = MultilevelSensorCc::decode(&bytes).unwrap();
        match back {
            MultilevelSensorCc::Report {
                sensor_type,
                scale,
                precision,
                value,
            } => {
                assert_eq!(sensor_type, 0x01);
                assert_eq!(scale, 0);
                assert_eq!(precision, 2);
                assert!((value - 21.50).abs() < 0.001);
            }
            _ => panic!("expected Report"),
        }
    }

    #[test]
    fn humidity_report_round_trip() {
        let cmd = MultilevelSensorCc::Report {
            sensor_type: SensorType::Humidity as u8,
            scale: 0,
            precision: 1,
            value: 42.5,
        };
        let bytes = cmd.encode();
        let back = MultilevelSensorCc::decode(&bytes).unwrap();
        match back {
            MultilevelSensorCc::Report { value, .. } => {
                assert!((value - 42.5).abs() < 0.001);
            }
            _ => panic!("expected Report"),
        }
    }

    #[test]
    fn get_with_type_and_scale_encodes_scale_bits() {
        let cmd = MultilevelSensorCc::Get {
            sensor_type: Some(SensorType::Temperature as u8),
            scale: Some(1),
        };
        let bytes = cmd.encode();
        assert_eq!(bytes[0], 0x31);
        assert_eq!(bytes[1], 0x04);
        assert_eq!(bytes[2], 0x01);
        // scale = 1 << 3 = 0x08
        assert_eq!(bytes[3], 0x08);
    }

    #[test]
    fn empty_get_round_trip() {
        let cmd = MultilevelSensorCc::Get {
            sensor_type: None,
            scale: None,
        };
        let bytes = cmd.encode();
        assert_eq!(bytes.as_ref(), &[0x31, 0x04]);
        assert_eq!(MultilevelSensorCc::decode(&bytes).unwrap(), cmd);
    }

    #[test]
    fn sensor_scale_for_type_decodes_known() {
        assert_eq!(
            SensorScale::for_type(SensorType::Temperature, 0),
            SensorScale::Celsius
        );
        assert_eq!(
            SensorScale::for_type(SensorType::Temperature, 1),
            SensorScale::Fahrenheit
        );
        assert_eq!(
            SensorScale::for_type(SensorType::Humidity, 0),
            SensorScale::Percent
        );
        assert_eq!(
            SensorScale::for_type(SensorType::Illuminance, 1),
            SensorScale::Lux
        );
    }
}
