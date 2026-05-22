// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Minimal deCONZ command surface — dresden-elektronik serial protocol §4.
//!
//! Frame layout (post-SLIP unstuff):
//!
//! ```text
//!   octet 0   : command id
//!   octet 1   : sequence
//!   octet 2   : status (0x00 success on responses)
//!   octets 3..4 : frame length (LE u16)  — total frame length incl. these fields
//!   octets 5.. : command payload
//!   last 2 bytes: CRC-16 (CCITT, see ezsp::ash::crc_ccitt) over all preceding bytes
//! ```
//!
//! Phase 1 only models the commands we actually issue during
//! coordinator init: version, read-parameter, write-parameter (channel
//! mask + PAN ID), permit-join, and APS-DATA-INDICATION callback ingestion.

use crate::error::{Result, ZigbeeError};
use crate::ezsp::ash::crc_ccitt;

/// deCONZ command IDs — public protocol document.
pub mod command_id {
    /// 0x0d — version request/response.
    pub const VERSION: u8 = 0x0d;
    /// 0x0a — read parameter (channel, PAN ID, extended PAN ID, network key, …).
    pub const READ_PARAMETER: u8 = 0x0a;
    /// 0x0b — write parameter.
    pub const WRITE_PARAMETER: u8 = 0x0b;
    /// 0x14 — permit-join.
    pub const PERMIT_JOIN: u8 = 0x14;
    /// 0x17 — APS-DATA-INDICATION (inbound ZCL frame).
    pub const APS_DATA_INDICATION: u8 = 0x17;
}

/// Parameter IDs for `READ_PARAMETER` / `WRITE_PARAMETER` — public.
pub mod parameter_id {
    /// Coordinator current channel.
    pub const CHANNEL: u8 = 0x1c;
    /// PAN ID.
    pub const PAN_ID: u8 = 0x05;
    /// Extended PAN ID.
    pub const EXTENDED_PAN_ID: u8 = 0x08;
}

/// Outbound deCONZ command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeconzCommand {
    /// version().
    Version,
    /// read-parameter(parameter_id).
    ReadParameter { parameter_id: u8 },
    /// write-parameter(parameter_id, value).
    WriteParameter { parameter_id: u8, value: Vec<u8> },
    /// permit-join(seconds).
    PermitJoin { seconds: u8 },
}

impl DeconzCommand {
    /// Command ID for this command.
    #[must_use]
    pub const fn command_id(&self) -> u8 {
        match self {
            Self::Version => command_id::VERSION,
            Self::ReadParameter { .. } => command_id::READ_PARAMETER,
            Self::WriteParameter { .. } => command_id::WRITE_PARAMETER,
            Self::PermitJoin { .. } => command_id::PERMIT_JOIN,
        }
    }

    /// Encode the command-specific payload (the bytes after frame length).
    #[must_use]
    pub fn encode_payload(&self) -> Vec<u8> {
        match self {
            // Outgoing version() carries 2 placeholder bytes; the dongle
            // overwrites these in the response with major/minor. The 2-byte
            // shape keeps encode/decode symmetric for the VERSION arm.
            Self::Version => vec![0x00, 0x00],
            Self::ReadParameter { parameter_id } => {
                // payload length LE u16 (==1), then parameter id.
                vec![0x01, 0x00, *parameter_id]
            }
            Self::WriteParameter {
                parameter_id,
                value,
            } => {
                let mut p = Vec::with_capacity(3 + value.len());
                let l = u16::try_from(value.len() + 1).unwrap_or(u16::MAX);
                p.extend_from_slice(&l.to_le_bytes());
                p.push(*parameter_id);
                p.extend_from_slice(value);
                p
            }
            Self::PermitJoin { seconds } => vec![*seconds, 0x00],
        }
    }

    /// Encode the full deCONZ frame (header + payload + CRC), pre-SLIP.
    #[must_use]
    pub fn to_frame_bytes(&self, sequence: u8) -> Vec<u8> {
        let payload = self.encode_payload();
        let frame_len = u16::try_from(5 + payload.len() + 2).unwrap_or(u16::MAX);
        let mut buf = Vec::with_capacity(usize::from(frame_len));
        buf.push(self.command_id());
        buf.push(sequence);
        buf.push(0x00); // status placeholder for outgoing
        buf.extend_from_slice(&frame_len.to_le_bytes());
        buf.extend_from_slice(&payload);
        let crc = crc_ccitt(&buf);
        buf.extend_from_slice(&crc.to_be_bytes());
        buf
    }
}

/// Inbound deCONZ response (post-SLIP unstuff, after CRC validation).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeconzResponse {
    /// version() response — major/minor stack version bytes.
    Version {
        sequence: u8,
        status: u8,
        major: u8,
        minor: u8,
    },
    /// read-parameter response (parameter id + raw value bytes).
    ReadParameter {
        sequence: u8,
        status: u8,
        parameter_id: u8,
        value: Vec<u8>,
    },
    /// write-parameter response (status only).
    WriteParameter {
        sequence: u8,
        status: u8,
        parameter_id: u8,
    },
    /// permit-join response (status only).
    PermitJoin { sequence: u8, status: u8 },
    /// APS-DATA-INDICATION — an inbound APS frame from a device.
    ApsDataIndication {
        sequence: u8,
        status: u8,
        payload: Vec<u8>,
    },
    /// Frame whose command id is outside Phase 1 — kept opaque.
    Unknown {
        command_id: u8,
        sequence: u8,
        status: u8,
        payload: Vec<u8>,
    },
}

impl DeconzResponse {
    /// Decode a complete deCONZ frame (post-SLIP unstuff).
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Integrity`] on CRC mismatch / length mismatch,
    /// or [`ZigbeeError::Truncated`] for short input.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 7 {
            return Err(ZigbeeError::Truncated {
                need: 7,
                have: bytes.len(),
            });
        }
        // Last 2 bytes are CRC (big-endian here — matches ASH convention).
        let (data, crc_bytes) = bytes.split_at(bytes.len() - 2);
        let expected = u16::from_be_bytes([crc_bytes[0], crc_bytes[1]]);
        let observed = crc_ccitt(data);
        if expected != observed {
            return Err(ZigbeeError::Integrity("deconz crc mismatch"));
        }
        let command_id = data[0];
        let sequence = data[1];
        let status = data[2];
        let frame_len = u16::from_le_bytes([data[3], data[4]]);
        if usize::from(frame_len) != bytes.len() {
            return Err(ZigbeeError::Integrity("deconz frame length mismatch"));
        }
        let payload = data[5..].to_vec();
        Ok(match command_id {
            command_id::VERSION => {
                if payload.len() < 2 {
                    return Err(ZigbeeError::Truncated {
                        need: 2,
                        have: payload.len(),
                    });
                }
                Self::Version {
                    sequence,
                    status,
                    major: payload[0],
                    minor: payload[1],
                }
            }
            command_id::READ_PARAMETER => {
                // Payload: LE u16 length + parameter id + value bytes.
                if payload.len() < 3 {
                    return Err(ZigbeeError::Truncated {
                        need: 3,
                        have: payload.len(),
                    });
                }
                let value_len = u16::from_le_bytes([payload[0], payload[1]]);
                if payload.len() < 2 + usize::from(value_len) {
                    return Err(ZigbeeError::Truncated {
                        need: 2 + usize::from(value_len),
                        have: payload.len(),
                    });
                }
                let parameter_id = payload[2];
                let value = payload[3..2 + usize::from(value_len)].to_vec();
                Self::ReadParameter {
                    sequence,
                    status,
                    parameter_id,
                    value,
                }
            }
            command_id::WRITE_PARAMETER => {
                if payload.len() < 3 {
                    return Err(ZigbeeError::Truncated {
                        need: 3,
                        have: payload.len(),
                    });
                }
                Self::WriteParameter {
                    sequence,
                    status,
                    parameter_id: payload[2],
                }
            }
            command_id::PERMIT_JOIN => Self::PermitJoin { sequence, status },
            command_id::APS_DATA_INDICATION => Self::ApsDataIndication {
                sequence,
                status,
                payload,
            },
            other => Self::Unknown {
                command_id: other,
                sequence,
                status,
                payload,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_command_frame_round_trips() {
        let cmd = DeconzCommand::Version;
        let bytes = cmd.to_frame_bytes(0x05);
        let resp = DeconzResponse::decode(&bytes).unwrap();
        // Our encoder writes 0 status, so decode comes back as version-shaped
        // payload of all zeros (the response handler treats it as version).
        if let DeconzResponse::Version {
            sequence, status, ..
        } = resp
        {
            assert_eq!(sequence, 0x05);
            assert_eq!(status, 0x00);
        } else {
            panic!("expected Version, got {resp:?}");
        }
    }

    #[test]
    fn read_parameter_command_frame_round_trips() {
        let cmd = DeconzCommand::ReadParameter {
            parameter_id: parameter_id::CHANNEL,
        };
        let bytes = cmd.to_frame_bytes(0x10);
        // Decode succeeds — read-parameter "value" is empty on the outgoing side.
        let resp = DeconzResponse::decode(&bytes).unwrap();
        match resp {
            DeconzResponse::ReadParameter {
                sequence,
                parameter_id,
                ..
            } => {
                assert_eq!(sequence, 0x10);
                assert_eq!(parameter_id, parameter_id::CHANNEL);
            }
            other => panic!("expected ReadParameter, got {other:?}"),
        }
    }

    #[test]
    fn write_parameter_command_frame_round_trips() {
        let cmd = DeconzCommand::WriteParameter {
            parameter_id: parameter_id::CHANNEL,
            value: vec![15],
        };
        let bytes = cmd.to_frame_bytes(0x11);
        let resp = DeconzResponse::decode(&bytes).unwrap();
        match resp {
            DeconzResponse::WriteParameter {
                sequence,
                parameter_id,
                ..
            } => {
                assert_eq!(sequence, 0x11);
                assert_eq!(parameter_id, parameter_id::CHANNEL);
            }
            other => panic!("expected WriteParameter, got {other:?}"),
        }
    }

    #[test]
    fn permit_join_command_frame_round_trips() {
        let cmd = DeconzCommand::PermitJoin { seconds: 60 };
        let bytes = cmd.to_frame_bytes(0x12);
        let resp = DeconzResponse::decode(&bytes).unwrap();
        match resp {
            DeconzResponse::PermitJoin { sequence, status } => {
                assert_eq!(sequence, 0x12);
                assert_eq!(status, 0x00);
            }
            other => panic!("expected PermitJoin, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_crc_corruption() {
        let cmd = DeconzCommand::Version;
        let mut bytes = cmd.to_frame_bytes(0x01);
        // Flip a CRC byte.
        *bytes.last_mut().unwrap() ^= 0x01;
        assert!(matches!(
            DeconzResponse::decode(&bytes),
            Err(ZigbeeError::Integrity(_))
        ));
    }

    #[test]
    fn decode_rejects_truncated() {
        assert!(matches!(
            DeconzResponse::decode(&[0x00, 0x00]),
            Err(ZigbeeError::Truncated { .. })
        ));
    }
}
