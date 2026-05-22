// SPDX-License-Identifier: Apache-2.0
//! `MultilevelSwitchCC` — Get / Set / Report / StartLevelChange / StopLevelChange.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/cc/src/cc/MultilevelSwitchCC.ts

use bytes::{BufMut, Bytes, BytesMut};

use super::CommandClassId;
use crate::error::{ZwaveError, ZwaveResult};

/// Command discriminator.
///
/// # Upstream: `_Types.ts::MultilevelSwitchCommand`
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MultilevelSwitchCommand {
    /// `Set`.
    Set = 0x01,
    /// `Get`.
    Get = 0x02,
    /// `Report`.
    Report = 0x03,
    /// `StartLevelChange`.
    StartLevelChange = 0x04,
    /// `StopLevelChange`.
    StopLevelChange = 0x05,
}

/// Direction for `StartLevelChange`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LevelChangeDirection {
    /// Up.
    Up = 0x00,
    /// Down.
    Down = 0x01,
}

/// Multilevel Switch CC payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultilevelSwitchCc {
    /// `Get` — empty.
    Get,
    /// `Set` with target level (0..99) + optional duration.
    Set {
        /// Target level (0..99 or 0xff legacy max).
        target_value: u8,
        /// Optional duration byte.
        duration: Option<u8>,
    },
    /// `Report` from the node.
    Report {
        /// Current level.
        current_value: u8,
        /// V4+ target level.
        target_value: Option<u8>,
        /// V4+ remaining duration.
        duration: Option<u8>,
    },
    /// `StartLevelChange`.
    StartLevelChange {
        /// Up or down.
        direction: LevelChangeDirection,
        /// `ignoreStartLevel` flag — when true, the node ignores
        /// `start_level` and uses its current.
        ignore_start_level: bool,
        /// Start level if the flag above is false.
        start_level: u8,
        /// Optional duration override.
        duration: Option<u8>,
    },
    /// `StopLevelChange` — empty.
    StopLevelChange,
}

impl MultilevelSwitchCc {
    /// Encode.
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u8(CommandClassId::MultilevelSwitch.as_u8());
        match self {
            Self::Get => buf.put_u8(MultilevelSwitchCommand::Get as u8),
            Self::Set {
                target_value,
                duration,
            } => {
                buf.put_u8(MultilevelSwitchCommand::Set as u8);
                buf.put_u8(*target_value);
                if let Some(d) = duration {
                    buf.put_u8(*d);
                }
            }
            Self::Report {
                current_value,
                target_value,
                duration,
            } => {
                buf.put_u8(MultilevelSwitchCommand::Report as u8);
                buf.put_u8(*current_value);
                if let (Some(t), Some(d)) = (target_value, duration) {
                    buf.put_u8(*t);
                    buf.put_u8(*d);
                }
            }
            Self::StartLevelChange {
                direction,
                ignore_start_level,
                start_level,
                duration,
            } => {
                buf.put_u8(MultilevelSwitchCommand::StartLevelChange as u8);
                // Bit 6 = direction (0 = up, 1 = down)
                // Bit 5 = ignore start level
                let mut flags: u8 = 0;
                if *direction == LevelChangeDirection::Down {
                    flags |= 0b0100_0000;
                }
                if *ignore_start_level {
                    flags |= 0b0010_0000;
                }
                buf.put_u8(flags);
                buf.put_u8(*start_level);
                if let Some(d) = duration {
                    buf.put_u8(*d);
                }
            }
            Self::StopLevelChange => buf.put_u8(MultilevelSwitchCommand::StopLevelChange as u8),
        }
        buf.freeze()
    }

    /// Decode.
    ///
    /// # Errors
    /// Returns [`ZwaveError::PacketFormat`] on truncation / unknown commands.
    pub fn decode(data: &[u8]) -> ZwaveResult<Self> {
        if data.len() < 2 {
            return Err(ZwaveError::PacketFormat(
                "MultilevelSwitchCC: payload shorter than 2 bytes".into(),
            ));
        }
        if data[0] != CommandClassId::MultilevelSwitch.as_u8() {
            return Err(ZwaveError::PacketFormat(format!(
                "MultilevelSwitchCC: leading byte 0x{:02x} != 0x26",
                data[0]
            )));
        }
        match data[1] {
            0x01 => {
                if data.len() < 3 {
                    return Err(ZwaveError::PacketFormat(
                        "MultilevelSwitchCCSet: missing target".into(),
                    ));
                }
                Ok(Self::Set {
                    target_value: data[2],
                    duration: data.get(3).copied(),
                })
            }
            0x02 => Ok(Self::Get),
            0x03 => {
                if data.len() < 3 {
                    return Err(ZwaveError::PacketFormat(
                        "MultilevelSwitchCCReport: missing current".into(),
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
            0x04 => {
                if data.len() < 4 {
                    return Err(ZwaveError::PacketFormat(
                        "StartLevelChange: missing fields".into(),
                    ));
                }
                let flags = data[2];
                let direction = if flags & 0b0100_0000 != 0 {
                    LevelChangeDirection::Down
                } else {
                    LevelChangeDirection::Up
                };
                let ignore_start_level = flags & 0b0010_0000 != 0;
                let start_level = data[3];
                Ok(Self::StartLevelChange {
                    direction,
                    ignore_start_level,
                    start_level,
                    duration: data.get(4).copied(),
                })
            }
            0x05 => Ok(Self::StopLevelChange),
            other => Err(ZwaveError::PacketFormat(format!(
                "MultilevelSwitchCC: unknown command 0x{other:02x}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_round_trip() {
        let cmd = MultilevelSwitchCc::Set {
            target_value: 42,
            duration: Some(0x05),
        };
        let bytes = cmd.encode();
        assert_eq!(bytes.as_ref(), &[0x26, 0x01, 42, 0x05]);
        assert_eq!(MultilevelSwitchCc::decode(&bytes).unwrap(), cmd);
    }

    #[test]
    fn start_level_change_down_round_trip() {
        let cmd = MultilevelSwitchCc::StartLevelChange {
            direction: LevelChangeDirection::Down,
            ignore_start_level: true,
            start_level: 0,
            duration: Some(0x05),
        };
        let bytes = cmd.encode();
        assert_eq!(bytes[0], 0x26);
        assert_eq!(bytes[1], 0x04);
        // flags = 0b0110_0000 = 0x60 (down + ignore_start_level)
        assert_eq!(bytes[2], 0x60);
        let back = MultilevelSwitchCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn stop_level_change_round_trip() {
        let bytes = MultilevelSwitchCc::StopLevelChange.encode();
        assert_eq!(bytes.as_ref(), &[0x26, 0x05]);
        assert_eq!(
            MultilevelSwitchCc::decode(&bytes).unwrap(),
            MultilevelSwitchCc::StopLevelChange
        );
    }

    #[test]
    fn report_v4_round_trip() {
        let cmd = MultilevelSwitchCc::Report {
            current_value: 50,
            target_value: Some(99),
            duration: Some(0xfe),
        };
        let bytes = cmd.encode();
        let back = MultilevelSwitchCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }
}
