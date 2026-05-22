// SPDX-License-Identifier: Apache-2.0
//! `ConfigurationCC` — Get / Set / Report.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/cc/src/cc/ConfigurationCC.ts
//!
//! Vendor-defined parameter store. Each parameter is keyed by a 1-byte
//! (V1..V3) or 2-byte (V4+) number; values are 1/2/4 bytes, signed or
//! unsigned. Phase 1 covers V1 (1-byte param + variable value).

use bytes::{BufMut, Bytes, BytesMut};

use super::CommandClassId;
use crate::error::{ZwaveError, ZwaveResult};

/// Command discriminator.
///
/// # Upstream: `_Types.ts::ConfigurationCommand`
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ConfigurationCommand {
    /// `Set`.
    Set = 0x04,
    /// `Get`.
    Get = 0x05,
    /// `Report`.
    Report = 0x06,
}

/// Configuration CC payloads (V1).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigurationCc {
    /// `Get` for a single parameter number.
    Get {
        /// Parameter number (V1, 1 byte).
        parameter: u8,
    },
    /// `Set` for a single parameter.
    Set {
        /// Parameter number.
        parameter: u8,
        /// Whether to set this parameter to its factory default.
        default: bool,
        /// Value size (1, 2 or 4 bytes).
        size: u8,
        /// Raw value bytes, MSB-first.
        value: Bytes,
    },
    /// `Report` of one parameter.
    Report {
        /// Parameter number.
        parameter: u8,
        /// Value size (1, 2 or 4 bytes).
        size: u8,
        /// Raw value bytes, MSB-first.
        value: Bytes,
    },
}

fn size_byte(size: u8, default: bool) -> u8 {
    // Upstream: bit 7 = default, bits 0..2 = size.
    let mut b = size & 0b0000_0111;
    if default {
        b |= 0b1000_0000;
    }
    b
}

impl ConfigurationCc {
    /// Encode.
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u8(CommandClassId::Configuration.as_u8());
        match self {
            Self::Get { parameter } => {
                buf.put_u8(ConfigurationCommand::Get as u8);
                buf.put_u8(*parameter);
            }
            Self::Set {
                parameter,
                default,
                size,
                value,
            } => {
                buf.put_u8(ConfigurationCommand::Set as u8);
                buf.put_u8(*parameter);
                buf.put_u8(size_byte(*size, *default));
                buf.put_slice(value);
            }
            Self::Report {
                parameter,
                size,
                value,
            } => {
                buf.put_u8(ConfigurationCommand::Report as u8);
                buf.put_u8(*parameter);
                buf.put_u8(size_byte(*size, false));
                buf.put_slice(value);
            }
        }
        buf.freeze()
    }

    /// Decode.
    ///
    /// # Errors
    /// Returns [`ZwaveError::PacketFormat`] for invalid framing.
    pub fn decode(data: &[u8]) -> ZwaveResult<Self> {
        if data.len() < 2 {
            return Err(ZwaveError::PacketFormat(
                "ConfigurationCC: payload shorter than 2 bytes".into(),
            ));
        }
        if data[0] != CommandClassId::Configuration.as_u8() {
            return Err(ZwaveError::PacketFormat(format!(
                "ConfigurationCC: leading byte 0x{:02x} != 0x70",
                data[0]
            )));
        }
        match data[1] {
            0x05 => {
                if data.len() < 3 {
                    return Err(ZwaveError::PacketFormat(
                        "ConfigurationCCGet: missing parameter".into(),
                    ));
                }
                Ok(Self::Get { parameter: data[2] })
            }
            0x04 => {
                if data.len() < 4 {
                    return Err(ZwaveError::PacketFormat(
                        "ConfigurationCCSet: missing header".into(),
                    ));
                }
                let parameter = data[2];
                let size_byte = data[3];
                let default = size_byte & 0b1000_0000 != 0;
                let size = size_byte & 0b0000_0111;
                if !default && data.len() < 4 + usize::from(size) {
                    return Err(ZwaveError::PacketFormat(
                        "ConfigurationCCSet: value truncated".into(),
                    ));
                }
                let value = if default {
                    Bytes::new()
                } else {
                    Bytes::copy_from_slice(&data[4..4 + usize::from(size)])
                };
                Ok(Self::Set {
                    parameter,
                    default,
                    size,
                    value,
                })
            }
            0x06 => {
                if data.len() < 4 {
                    return Err(ZwaveError::PacketFormat(
                        "ConfigurationCCReport: missing header".into(),
                    ));
                }
                let parameter = data[2];
                let size_byte = data[3];
                let size = size_byte & 0b0000_0111;
                if data.len() < 4 + usize::from(size) {
                    return Err(ZwaveError::PacketFormat(
                        "ConfigurationCCReport: value truncated".into(),
                    ));
                }
                let value = Bytes::copy_from_slice(&data[4..4 + usize::from(size)]);
                Ok(Self::Report {
                    parameter,
                    size,
                    value,
                })
            }
            other => Err(ZwaveError::PacketFormat(format!(
                "ConfigurationCC: unknown command 0x{other:02x}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_round_trip() {
        let cmd = ConfigurationCc::Get { parameter: 12 };
        let bytes = cmd.encode();
        assert_eq!(bytes.as_ref(), &[0x70, 0x05, 12]);
        let back = ConfigurationCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn set_4_byte_value_round_trip() {
        let cmd = ConfigurationCc::Set {
            parameter: 99,
            default: false,
            size: 4,
            value: Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef]),
        };
        let bytes = cmd.encode();
        // Header byte: size=4, default=false → 0x04
        assert_eq!(
            bytes.as_ref(),
            &[0x70, 0x04, 99, 0x04, 0xde, 0xad, 0xbe, 0xef]
        );
        let back = ConfigurationCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn set_default_skips_value() {
        let cmd = ConfigurationCc::Set {
            parameter: 5,
            default: true,
            size: 0,
            value: Bytes::new(),
        };
        let bytes = cmd.encode();
        // default flag set, size 0
        assert_eq!(bytes.as_ref(), &[0x70, 0x04, 5, 0x80]);
        let back = ConfigurationCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn report_2_byte_round_trip() {
        let cmd = ConfigurationCc::Report {
            parameter: 7,
            size: 2,
            value: Bytes::from_static(&[0x01, 0x00]),
        };
        let bytes = cmd.encode();
        let back = ConfigurationCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }
}
