// SPDX-License-Identifier: Apache-2.0
//! `NotificationCC` — Get / Report.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/cc/src/cc/NotificationCC.ts
//!
//! Notification CC carries event-style alerts: motion detected, door/window
//! opened, smoke alarm, etc. Phase 1 covers the type discriminators the
//! headline persona's home actually fires.

use bytes::{BufMut, Bytes, BytesMut};

use super::CommandClassId;
use crate::error::{ZwaveError, ZwaveResult};

/// Command discriminator.
///
/// # Upstream: `_Types.ts::NotificationCommand`
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NotificationCommand {
    /// `Get` — V3+ uses a single notification-type byte.
    Get = 0x04,
    /// `Report` — node -> host.
    Report = 0x05,
}

/// Notification-type byte (Phase 1 set).
///
/// # Upstream: `NotificationCC.ts` (`Notification Type Registry`).
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NotificationType {
    /// 0x01 — Smoke alarm.
    Smoke = 0x01,
    /// 0x02 — CO alarm.
    CarbonMonoxide = 0x02,
    /// 0x05 — Water leak.
    Water = 0x05,
    /// 0x06 — Access control (lock / door / window).
    AccessControl = 0x06,
    /// 0x07 — Home security (motion / tamper / glass-break).
    HomeSecurity = 0x07,
    /// 0x09 — System (fault, hardware failure).
    System = 0x09,
}

impl NotificationType {
    /// Decode the type byte. Returns `None` for Phase 1-out-of-scope values.
    #[must_use]
    pub const fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Smoke),
            0x02 => Some(Self::CarbonMonoxide),
            0x05 => Some(Self::Water),
            0x06 => Some(Self::AccessControl),
            0x07 => Some(Self::HomeSecurity),
            0x09 => Some(Self::System),
            _ => None,
        }
    }
}

/// Notification CC payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotificationCc {
    /// `Get` for a specific type / event.
    Get {
        /// Alarm type byte (legacy V1).
        alarm_type: u8,
        /// V3+ notification type.
        notification_type: u8,
        /// V3+ event filter.
        event: u8,
    },
    /// `Report` from the node.
    Report {
        /// V1 alarm type byte (0 if not used).
        alarm_type: u8,
        /// V1 alarm level byte (0 if not used).
        alarm_level: u8,
        /// V3+ notification status (0xff = on, 0x00 = off).
        notification_status: u8,
        /// V3+ notification type.
        notification_type: u8,
        /// V3+ event byte.
        event: u8,
        /// Variable-length parameters payload.
        parameters: Bytes,
    },
}

impl NotificationCc {
    /// Encode.
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u8(CommandClassId::Notification.as_u8());
        match self {
            Self::Get {
                alarm_type,
                notification_type,
                event,
            } => {
                buf.put_u8(NotificationCommand::Get as u8);
                buf.put_u8(*alarm_type);
                buf.put_u8(*notification_type);
                buf.put_u8(*event);
            }
            Self::Report {
                alarm_type,
                alarm_level,
                notification_status,
                notification_type,
                event,
                parameters,
            } => {
                buf.put_u8(NotificationCommand::Report as u8);
                buf.put_u8(*alarm_type);
                buf.put_u8(*alarm_level);
                buf.put_u8(0); // reserved
                buf.put_u8(*notification_status);
                buf.put_u8(*notification_type);
                buf.put_u8(*event);
                // Bits 0..4 = event-parameter length.
                #[allow(clippy::cast_possible_truncation)]
                let plen = parameters.len() as u8 & 0b0001_1111;
                buf.put_u8(plen);
                buf.put_slice(parameters);
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
                "NotificationCC: payload shorter than 2 bytes".into(),
            ));
        }
        if data[0] != CommandClassId::Notification.as_u8() {
            return Err(ZwaveError::PacketFormat(format!(
                "NotificationCC: leading byte 0x{:02x} != 0x71",
                data[0]
            )));
        }
        match data[1] {
            0x04 => {
                if data.len() < 5 {
                    return Err(ZwaveError::PacketFormat(
                        "NotificationCCGet: missing fields".into(),
                    ));
                }
                Ok(Self::Get {
                    alarm_type: data[2],
                    notification_type: data[3],
                    event: data[4],
                })
            }
            0x05 => {
                if data.len() < 9 {
                    return Err(ZwaveError::PacketFormat(
                        "NotificationCCReport: missing fields".into(),
                    ));
                }
                let alarm_type = data[2];
                let alarm_level = data[3];
                // data[4] reserved
                let notification_status = data[5];
                let notification_type = data[6];
                let event = data[7];
                let plen = usize::from(data[8] & 0b0001_1111);
                if data.len() < 9 + plen {
                    return Err(ZwaveError::PacketFormat(
                        "NotificationCCReport: parameters truncated".into(),
                    ));
                }
                let parameters = Bytes::copy_from_slice(&data[9..9 + plen]);
                Ok(Self::Report {
                    alarm_type,
                    alarm_level,
                    notification_status,
                    notification_type,
                    event,
                    parameters,
                })
            }
            other => Err(ZwaveError::PacketFormat(format!(
                "NotificationCC: unknown command 0x{other:02x}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_security_motion_report_round_trips() {
        let cmd = NotificationCc::Report {
            alarm_type: 0,
            alarm_level: 0,
            notification_status: 0xff,
            notification_type: NotificationType::HomeSecurity as u8,
            event: 0x08, // Motion detected
            parameters: Bytes::new(),
        };
        let bytes = cmd.encode();
        let back = NotificationCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn access_control_door_report_with_params_round_trips() {
        let cmd = NotificationCc::Report {
            alarm_type: 0,
            alarm_level: 0,
            notification_status: 0xff,
            notification_type: NotificationType::AccessControl as u8,
            event: 0x16, // Window/Door is open
            parameters: Bytes::from_static(&[0x02]),
        };
        let bytes = cmd.encode();
        let back = NotificationCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn get_round_trips() {
        let cmd = NotificationCc::Get {
            alarm_type: 0,
            notification_type: NotificationType::HomeSecurity as u8,
            event: 0x00,
        };
        let bytes = cmd.encode();
        let back = NotificationCc::decode(&bytes).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn notification_type_round_trip() {
        for t in [
            NotificationType::Smoke,
            NotificationType::CarbonMonoxide,
            NotificationType::Water,
            NotificationType::AccessControl,
            NotificationType::HomeSecurity,
            NotificationType::System,
        ] {
            assert_eq!(NotificationType::from_u8(t as u8), Some(t));
        }
        assert_eq!(NotificationType::from_u8(0xff), None);
    }
}
