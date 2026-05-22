// SPDX-License-Identifier: Apache-2.0
//! `BinarySwitchCC` — Get / Set / Report.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/cc/src/cc/BinarySwitchCC.ts

use bytes::{BufMut, Bytes, BytesMut};

use super::CommandClassId;
use crate::error::{ZwaveError, ZwaveResult};

/// Command discriminator.
///
/// # Upstream: `_Types.ts::BinarySwitchCommand`
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BinarySwitchCommand {
    /// `Set`.
    Set = 0x01,
    /// `Get`.
    Get = 0x02,
    /// `Report`.
    Report = 0x03,
}

/// Binary Switch CC payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BinarySwitchCc {
    /// `Get` — empty payload.
    Get,
    /// `Set` with `target_value` (true / false).
    Set {
        /// Desired state.
        target_value: bool,
        /// Optional duration byte (V2+).
        duration: Option<u8>,
    },
    /// `Report` from the node.
    Report {
        /// Current state.
        current_value: bool,
        /// Target state (V2+).
        target_value: Option<bool>,
        /// Duration (V2+).
        duration: Option<u8>,
    },
}

const ON: u8 = 0xff;
const OFF: u8 = 0x00;

/// Convert a bool to the wire byte. Upstream uses 0x00 for off and 0xff for on
/// (intermediate values are reserved).
const fn bool_to_byte(b: bool) -> u8 {
    if b { ON } else { OFF }
}

/// Convert a wire byte to a bool. Anything non-zero is "on" (matches
/// upstream's `parseMaybeBoolean`).
const fn byte_to_bool(b: u8) -> bool {
    b != 0
}

impl BinarySwitchCc {
    /// Encode to `CC_ID | CMD | payload`.
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u8(CommandClassId::BinarySwitch.as_u8());
        match self {
            Self::Get => buf.put_u8(BinarySwitchCommand::Get as u8),
            Self::Set {
                target_value,
                duration,
            } => {
                buf.put_u8(BinarySwitchCommand::Set as u8);
                buf.put_u8(bool_to_byte(*target_value));
                if let Some(d) = duration {
                    buf.put_u8(*d);
                }
            }
            Self::Report {
                current_value,
                target_value,
                duration,
            } => {
                buf.put_u8(BinarySwitchCommand::Report as u8);
                buf.put_u8(bool_to_byte(*current_value));
                if let (Some(t), Some(d)) = (target_value, duration) {
                    buf.put_u8(bool_to_byte(*t));
                    buf.put_u8(*d);
                }
            }
        }
        buf.freeze()
    }

    /// Decode from the wire bytes.
    ///
    /// # Errors
    /// Returns [`ZwaveError::PacketFormat`] for invalid framing.
    pub fn decode(data: &[u8]) -> ZwaveResult<Self> {
        if data.len() < 2 {
            return Err(ZwaveError::PacketFormat(
                "BinarySwitchCC: payload shorter than 2 bytes".into(),
            ));
        }
        if data[0] != CommandClassId::BinarySwitch.as_u8() {
            return Err(ZwaveError::PacketFormat(format!(
                "BinarySwitchCC: leading byte 0x{:02x} != 0x25",
                data[0]
            )));
        }
        match data[1] {
            0x01 => {
                if data.len() < 3 {
                    return Err(ZwaveError::PacketFormat(
                        "BinarySwitchCCSet: missing target".into(),
                    ));
                }
                let target_value = byte_to_bool(data[2]);
                let duration = data.get(3).copied();
                Ok(Self::Set {
                    target_value,
                    duration,
                })
            }
            0x02 => Ok(Self::Get),
            0x03 => {
                if data.len() < 3 {
                    return Err(ZwaveError::PacketFormat(
                        "BinarySwitchCCReport: missing current".into(),
                    ));
                }
                let current_value = byte_to_bool(data[2]);
                let (target_value, duration) = if data.len() >= 5 {
                    (Some(byte_to_bool(data[3])), Some(data[4]))
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
                "BinarySwitchCC: unknown command 0x{other:02x}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_encodes_as_ff() {
        let bytes = BinarySwitchCc::Set {
            target_value: true,
            duration: None,
        }
        .encode();
        assert_eq!(bytes.as_ref(), &[0x25, 0x01, 0xff]);
    }

    #[test]
    fn off_encodes_as_00() {
        let bytes = BinarySwitchCc::Set {
            target_value: false,
            duration: None,
        }
        .encode();
        assert_eq!(bytes.as_ref(), &[0x25, 0x01, 0x00]);
    }

    #[test]
    fn get_round_trip() {
        let bytes = BinarySwitchCc::Get.encode();
        assert_eq!(bytes.as_ref(), &[0x25, 0x02]);
        assert_eq!(BinarySwitchCc::decode(&bytes).unwrap(), BinarySwitchCc::Get);
    }

    #[test]
    fn report_round_trip_v2() {
        let cmd = BinarySwitchCc::Report {
            current_value: true,
            target_value: Some(false),
            duration: Some(0x05),
        };
        let bytes = cmd.encode();
        let back = BinarySwitchCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }
}
