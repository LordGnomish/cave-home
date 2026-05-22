// SPDX-License-Identifier: Apache-2.0
//! `BatteryCC` — Get / Report.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/cc/src/cc/BatteryCC.ts

use bytes::{BufMut, Bytes, BytesMut};

use super::CommandClassId;
use crate::error::{ZwaveError, ZwaveResult};

/// Command discriminator.
///
/// # Upstream: `_Types.ts::BatteryCommand`
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BatteryCommand {
    /// `Get`.
    Get = 0x02,
    /// `Report`.
    Report = 0x03,
}

/// Battery CC payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BatteryCc {
    /// `Get`.
    Get,
    /// `Report`.
    Report {
        /// Battery level (0..100, or 0xff = low battery flag).
        level: u8,
    },
}

impl BatteryCc {
    /// Encode.
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u8(CommandClassId::Battery.as_u8());
        match self {
            Self::Get => buf.put_u8(BatteryCommand::Get as u8),
            Self::Report { level } => {
                buf.put_u8(BatteryCommand::Report as u8);
                buf.put_u8(*level);
            }
        }
        buf.freeze()
    }

    /// Decode.
    ///
    /// # Errors
    /// Returns [`ZwaveError::PacketFormat`] for unknown commands.
    pub fn decode(data: &[u8]) -> ZwaveResult<Self> {
        if data.len() < 2 {
            return Err(ZwaveError::PacketFormat(
                "BatteryCC: payload shorter than 2 bytes".into(),
            ));
        }
        if data[0] != CommandClassId::Battery.as_u8() {
            return Err(ZwaveError::PacketFormat(format!(
                "BatteryCC: leading byte 0x{:02x} != 0x80",
                data[0]
            )));
        }
        match data[1] {
            0x02 => Ok(Self::Get),
            0x03 => {
                if data.len() < 3 {
                    return Err(ZwaveError::PacketFormat(
                        "BatteryCCReport: missing level".into(),
                    ));
                }
                Ok(Self::Report { level: data[2] })
            }
            other => Err(ZwaveError::PacketFormat(format!(
                "BatteryCC: unknown command 0x{other:02x}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_encodes_as_two_bytes() {
        assert_eq!(BatteryCc::Get.encode().as_ref(), &[0x80, 0x02]);
    }

    #[test]
    fn report_round_trip() {
        let cmd = BatteryCc::Report { level: 75 };
        let bytes = cmd.encode();
        assert_eq!(bytes.as_ref(), &[0x80, 0x03, 75]);
        assert_eq!(BatteryCc::decode(&bytes).unwrap(), cmd);
    }

    #[test]
    fn low_battery_sentinel_round_trips() {
        let cmd = BatteryCc::Report { level: 0xff };
        let bytes = cmd.encode();
        assert_eq!(BatteryCc::decode(&bytes).unwrap(), cmd);
    }
}
