// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! ZCL frame header — ZCL §2.6.
//!
//! Frame layout on the wire (little-endian, ZCL §2.6.1):
//!
//! ```text
//!   octet 0           : Frame control
//!   octets 1..2       : Manufacturer code (present iff frame_control.mfr_specific)
//!   octet 1 or 3      : Transaction sequence number
//!   octet 2 or 4      : Command identifier
//!   octets …          : Frame payload
//! ```
//!
//! Frame control byte breakdown (LSB-first per the spec figure):
//! - bits 0..1 — frame type (00 = profile-wide, 01 = cluster-specific)
//! - bit  2    — manufacturer specific (1 ⇒ mfr-code present)
//! - bit  3    — direction (0 = client→server, 1 = server→client)
//! - bit  4    — disable default response
//! - bits 5..7 — reserved (transmit as zero, ignore on receive)

use crate::error::{Result, ZigbeeError};

/// ZCL frame type — ZCL §2.6.2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameType {
    /// Profile-wide command (a Foundation command — Read/Write/Report…).
    ProfileWide,
    /// Cluster-specific command (e.g. Groups Add 0x00, OnOff Toggle 0x02).
    ClusterSpecific,
}

impl FrameType {
    /// Encode to the two-bit frame-type field.
    #[must_use]
    pub const fn to_bits(self) -> u8 {
        match self {
            Self::ProfileWide => 0b00,
            Self::ClusterSpecific => 0b01,
        }
    }

    /// Decode from the two-bit frame-type field. Unknown values are
    /// reported as integrity failures (ZCL §2.6.2 reserves bits 10 and 11).
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Integrity`] for reserved encodings.
    pub const fn from_bits(bits: u8) -> Result<Self> {
        match bits & 0b11 {
            0b00 => Ok(Self::ProfileWide),
            0b01 => Ok(Self::ClusterSpecific),
            _ => Err(ZigbeeError::Integrity("reserved zcl frame type")),
        }
    }
}

/// Direction of the ZCL command — ZCL §2.6.2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// Command is sent from a client to a server (Read Attributes from
    /// the coordinator to a bulb is `ClientToServer`).
    ClientToServer,
    /// Response or unsolicited report from server to client.
    ServerToClient,
}

impl Direction {
    /// Encode to the direction bit.
    #[must_use]
    pub const fn to_bit(self) -> u8 {
        match self {
            Self::ClientToServer => 0,
            Self::ServerToClient => 1,
        }
    }

    /// Decode from the direction bit (lowest bit only).
    #[must_use]
    pub const fn from_bit(bit: u8) -> Self {
        if bit & 1 == 0 {
            Self::ClientToServer
        } else {
            Self::ServerToClient
        }
    }
}

/// Optional manufacturer code — present iff [`ZclFrameControl::mfr_specific`].
pub type ManufacturerCode = u16;

/// Decoded ZCL frame-control byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZclFrameControl {
    /// Profile-wide vs cluster-specific.
    pub frame_type: FrameType,
    /// `true` ⇒ a 16-bit manufacturer code follows.
    pub mfr_specific: bool,
    /// Direction of the command.
    pub direction: Direction,
    /// `true` ⇒ the recipient must NOT send a default response.
    pub disable_default_response: bool,
}

impl ZclFrameControl {
    /// Construct a sensible profile-wide, client→server frame control.
    #[must_use]
    pub const fn profile_wide() -> Self {
        Self {
            frame_type: FrameType::ProfileWide,
            mfr_specific: false,
            direction: Direction::ClientToServer,
            disable_default_response: false,
        }
    }

    /// Construct a sensible cluster-specific, client→server frame control.
    #[must_use]
    pub const fn cluster_specific() -> Self {
        Self {
            frame_type: FrameType::ClusterSpecific,
            mfr_specific: false,
            direction: Direction::ClientToServer,
            disable_default_response: false,
        }
    }

    /// Encode to the single frame-control byte.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        let mut b: u8 = self.frame_type.to_bits();
        if self.mfr_specific {
            b |= 1 << 2;
        }
        b |= self.direction.to_bit() << 3;
        if self.disable_default_response {
            b |= 1 << 4;
        }
        b
    }

    /// Decode a frame-control byte.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Integrity`] if a reserved frame-type code is used.
    pub fn from_byte(b: u8) -> Result<Self> {
        let frame_type = FrameType::from_bits(b)?;
        Ok(Self {
            frame_type,
            mfr_specific: (b >> 2) & 1 == 1,
            direction: Direction::from_bit(b >> 3),
            disable_default_response: (b >> 4) & 1 == 1,
        })
    }
}

/// A decoded ZCL frame — header + raw payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZclFrame {
    /// Frame-control byte (decoded).
    pub control: ZclFrameControl,
    /// 16-bit manufacturer code, only present when `control.mfr_specific`.
    pub mfr_code: Option<ManufacturerCode>,
    /// Transaction sequence number (ZCL §2.6.4) — wraps mod 256.
    pub tsn: u8,
    /// ZCL command identifier.
    pub command_id: u8,
    /// Command-specific payload (still raw bytes; parsed per command).
    pub payload: Vec<u8>,
}

impl ZclFrame {
    /// Build a profile-wide client→server frame with the given command.
    #[must_use]
    pub fn profile_wide(tsn: u8, command_id: u8, payload: Vec<u8>) -> Self {
        Self {
            control: ZclFrameControl::profile_wide(),
            mfr_code: None,
            tsn,
            command_id,
            payload,
        }
    }

    /// Build a cluster-specific client→server frame with the given command.
    #[must_use]
    pub fn cluster_specific(tsn: u8, command_id: u8, payload: Vec<u8>) -> Self {
        Self {
            control: ZclFrameControl::cluster_specific(),
            mfr_code: None,
            tsn,
            command_id,
            payload,
        }
    }

    /// Encode the frame to its on-wire byte representation.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        // Header is 3 bytes (or 5 with mfr code), then payload follows.
        let mut out = Vec::with_capacity(3 + usize::from(self.control.mfr_specific) * 2 + self.payload.len());
        out.push(self.control.to_byte());
        if let Some(code) = self.mfr_code {
            out.push((code & 0xff) as u8);
            out.push((code >> 8) as u8);
        }
        out.push(self.tsn);
        out.push(self.command_id);
        out.extend_from_slice(&self.payload);
        out
    }

    /// Decode an on-wire ZCL frame.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] if `bytes` is too short, or
    /// [`ZigbeeError::Integrity`] for reserved frame-type bits.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(ZigbeeError::Truncated { need: 3, have: 0 });
        }
        let control = ZclFrameControl::from_byte(bytes[0])?;
        let mut i = 1;
        let mfr_code = if control.mfr_specific {
            if bytes.len() < i + 2 {
                return Err(ZigbeeError::Truncated {
                    need: i + 2,
                    have: bytes.len(),
                });
            }
            let lo = u16::from(bytes[i]);
            let hi = u16::from(bytes[i + 1]);
            i += 2;
            Some(lo | (hi << 8))
        } else {
            None
        };
        if bytes.len() < i + 2 {
            return Err(ZigbeeError::Truncated {
                need: i + 2,
                have: bytes.len(),
            });
        }
        let tsn = bytes[i];
        let command_id = bytes[i + 1];
        i += 2;
        let payload = bytes[i..].to_vec();
        Ok(Self {
            control,
            mfr_code,
            tsn,
            command_id,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_control_profile_wide_round_trip() {
        let fc = ZclFrameControl::profile_wide();
        assert_eq!(ZclFrameControl::from_byte(fc.to_byte()).unwrap(), fc);
    }

    #[test]
    fn frame_control_cluster_specific_round_trip() {
        let fc = ZclFrameControl::cluster_specific();
        assert_eq!(ZclFrameControl::from_byte(fc.to_byte()).unwrap(), fc);
    }

    #[test]
    fn frame_control_server_to_client_with_disable_response() {
        let fc = ZclFrameControl {
            frame_type: FrameType::ProfileWide,
            mfr_specific: false,
            direction: Direction::ServerToClient,
            disable_default_response: true,
        };
        assert_eq!(ZclFrameControl::from_byte(fc.to_byte()).unwrap(), fc);
        // direction bit 3 + disable bit 4 ⇒ 0b00011000 = 0x18
        assert_eq!(fc.to_byte(), 0x18);
    }

    #[test]
    fn frame_control_reserved_bits_rejected() {
        // bits 0..1 = 11 (reserved)
        let err = ZclFrameControl::from_byte(0b11).unwrap_err();
        assert!(matches!(err, ZigbeeError::Integrity(_)));
    }

    #[test]
    fn frame_encode_decode_round_trip_no_mfr() {
        let frame = ZclFrame::profile_wide(0x42, 0x00, vec![0x01, 0x02, 0x03, 0x04]);
        let bytes = frame.encode();
        let decoded = ZclFrame::decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn frame_encode_decode_round_trip_with_mfr() {
        let mut frame = ZclFrame::cluster_specific(0x10, 0xa1, vec![0xde, 0xad]);
        frame.control.mfr_specific = true;
        frame.mfr_code = Some(0x100b);
        let bytes = frame.encode();
        assert_eq!(bytes[0] & (1 << 2), 1 << 2, "mfr bit must be set");
        let decoded = ZclFrame::decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn frame_decode_truncated_returns_error() {
        let err = ZclFrame::decode(&[]).unwrap_err();
        assert!(matches!(err, ZigbeeError::Truncated { .. }));
        let err = ZclFrame::decode(&[0x00, 0x01]).unwrap_err();
        assert!(matches!(err, ZigbeeError::Truncated { .. }));
    }

    #[test]
    fn frame_command_id_and_tsn_at_expected_offsets() {
        let frame = ZclFrame::profile_wide(0x99, 0x0a, vec![]);
        let bytes = frame.encode();
        assert_eq!(bytes.len(), 3, "header without mfr is 3 bytes");
        assert_eq!(bytes[1], 0x99);
        assert_eq!(bytes[2], 0x0a);
    }
}
