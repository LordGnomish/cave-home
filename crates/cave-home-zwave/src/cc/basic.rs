// SPDX-License-Identifier: Apache-2.0
//! `BasicCC` — Get / Set / Report.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/cc/src/cc/BasicCC.ts
//!
//! `Basic` is the lowest-common-denominator command class — every Z-Wave
//! device understands it. Values are 0..99 or 0xff (= 99 / "max", legacy).

use bytes::{BufMut, Bytes, BytesMut};

use super::CommandClassId;
use crate::error::{ZwaveError, ZwaveResult};

/// Command discriminator (second byte of the CC payload).
///
/// # Upstream: `_Types.ts::BasicCommand`
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BasicCommand {
    /// `Set` — host -> node.
    Set = 0x01,
    /// `Get` — host -> node.
    Get = 0x02,
    /// `Report` — node -> host.
    Report = 0x03,
}

/// Basic CC encoded payload kinds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BasicCc {
    /// `BasicCCGet` — empty payload.
    Get,
    /// `BasicCCSet` with the target value (0..99 or 0xff).
    Set {
        /// Target value (0..99 = off..max, 0xff = "max").
        target_value: u8,
    },
    /// `BasicCCReport` with the value the node is reporting.
    Report {
        /// `currentValue` (0..99 or 0xff).
        current_value: u8,
        /// `targetValue` if the V2 payload is present.
        target_value: Option<u8>,
        /// `duration` byte if the V2 payload is present.
        duration: Option<u8>,
    },
}

impl BasicCc {
    /// Encode to the on-the-wire CC payload (`CC_ID | CMD | payload`).
    ///
    /// # Upstream: `BasicCC.ts::BasicCCSet.serialize` /
    /// `BasicCCReport.serialize` / `BasicCCGet`.
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u8(CommandClassId::Basic.as_u8());
        match self {
            Self::Get => {
                buf.put_u8(BasicCommand::Get as u8);
            }
            Self::Set { target_value } => {
                buf.put_u8(BasicCommand::Set as u8);
                buf.put_u8(*target_value);
            }
            Self::Report {
                current_value,
                target_value,
                duration,
            } => {
                buf.put_u8(BasicCommand::Report as u8);
                buf.put_u8(*current_value);
                if let (Some(t), Some(d)) = (target_value, duration) {
                    buf.put_u8(*t);
                    buf.put_u8(*d);
                }
            }
        }
        buf.freeze()
    }

    /// Decode from the on-the-wire CC payload.
    ///
    /// # Upstream: `BasicCC.ts::BasicCCSet.from` / `BasicCCReport.from`.
    ///
    /// # Errors
    /// Returns [`ZwaveError::PacketFormat`] for unknown commands or wrong
    /// payload lengths.
    pub fn decode(data: &[u8]) -> ZwaveResult<Self> {
        if data.len() < 2 {
            return Err(ZwaveError::PacketFormat(
                "BasicCC: payload shorter than 2 bytes".into(),
            ));
        }
        if data[0] != CommandClassId::Basic.as_u8() {
            return Err(ZwaveError::PacketFormat(format!(
                "BasicCC: leading byte 0x{:02x} != 0x20",
                data[0]
            )));
        }
        match data[1] {
            0x01 => {
                if data.len() < 3 {
                    return Err(ZwaveError::PacketFormat(
                        "BasicCCSet: missing target value".into(),
                    ));
                }
                Ok(Self::Set {
                    target_value: data[2],
                })
            }
            0x02 => Ok(Self::Get),
            0x03 => {
                if data.len() < 3 {
                    return Err(ZwaveError::PacketFormat(
                        "BasicCCReport: missing current value".into(),
                    ));
                }
                let current_value = data[2];
                let (target_value, duration) = if data.len() >= 5 {
                    (Some(data[3]), Some(data[4]))
                } else {
                    (None, None)
                };
                Ok(Self::Report {
                    current_value,
                    target_value,
                    duration,
                })
            }
            other => Err(ZwaveError::PacketFormat(format!(
                "BasicCC: unknown command 0x{other:02x}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_get_encodes_to_two_bytes() {
        let bytes = BasicCc::Get.encode();
        assert_eq!(bytes.as_ref(), &[0x20, 0x02]);
    }

    #[test]
    fn basic_set_round_trip() {
        let cmd = BasicCc::Set { target_value: 0xff };
        let bytes = cmd.encode();
        assert_eq!(bytes.as_ref(), &[0x20, 0x01, 0xff]);
        let back = BasicCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn basic_report_v1_round_trip() {
        let cmd = BasicCc::Report {
            current_value: 50,
            target_value: None,
            duration: None,
        };
        let bytes = cmd.encode();
        assert_eq!(bytes.as_ref(), &[0x20, 0x03, 50]);
        let back = BasicCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn basic_report_v2_round_trip() {
        let cmd = BasicCc::Report {
            current_value: 50,
            target_value: Some(99),
            duration: Some(0xfe),
        };
        let bytes = cmd.encode();
        assert_eq!(bytes.as_ref(), &[0x20, 0x03, 50, 99, 0xfe]);
        let back = BasicCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn decode_rejects_wrong_cc_id() {
        let err = BasicCc::decode(&[0x21, 0x02]).unwrap_err();
        assert!(matches!(err, ZwaveError::PacketFormat(_)));
    }

    #[test]
    fn decode_rejects_unknown_command() {
        let err = BasicCc::decode(&[0x20, 0x09]).unwrap_err();
        assert!(matches!(err, ZwaveError::PacketFormat(_)));
    }
}
