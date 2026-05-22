// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! EZSP application frame — Silicon Labs UG100 §3.
//!
//! EZSP frame format (host→NCP, EZSP v8+):
//!
//! ```text
//!   octet 0       : Sequence (incremented per outgoing command)
//!   octet 1       : Frame control low
//!   octet 2       : Frame control high
//!   octet 3..4    : Frame ID (LE u16)
//!   octets 5..    : Command parameters
//! ```
//!
//! Earlier EZSP versions used a 5-byte header with an 8-bit frame ID; we
//! target v8 because every supported Silicon Labs NCP (ZBDongle-E,
//! SLZB-06) ships with at least an EZSP-v8-capable firmware.

use crate::error::{Result, ZigbeeError};

/// Decoded EZSP frame-control field.
///
/// The bit layout is **publicly documented** in Silicon Labs UG100; for
/// Phase 1 we only need the direction + sleep flag bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EzspFrameControl {
    /// `false` ⇒ command from host to NCP; `true` ⇒ response/callback.
    pub response: bool,
    /// True ⇒ NCP requests host wake-up handling (UG100 §3.2.2).
    pub network_index_request: bool,
}

impl EzspFrameControl {
    /// Default outbound command framing.
    #[must_use]
    pub const fn command() -> Self {
        Self {
            response: false,
            network_index_request: false,
        }
    }

    /// Default response framing.
    #[must_use]
    pub const fn response() -> Self {
        Self {
            response: true,
            network_index_request: false,
        }
    }

    /// Encode to the two frame-control bytes (low / high).
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 2] {
        let lo: u8 = if self.response { 0x80 } else { 0x00 }
            | if self.network_index_request { 0x40 } else { 0x00 };
        // UG100 v8: hi byte carries EZSP frame format version.
        // 0x01 marks v8 extended format.
        let hi: u8 = 0x01;
        [lo, hi]
    }

    /// Decode from two on-wire bytes.
    #[must_use]
    pub const fn from_bytes(lo: u8, _hi: u8) -> Self {
        Self {
            response: (lo & 0x80) != 0,
            network_index_request: (lo & 0x40) != 0,
        }
    }
}

/// One EZSP application frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EzspFrame {
    /// Sequence number — host increments per command, NCP echoes per response.
    pub sequence: u8,
    /// Frame control bits.
    pub control: EzspFrameControl,
    /// 16-bit frame ID (e.g. 0x0000 version, 0x0017 networkInit).
    pub frame_id: u16,
    /// Command-specific parameter payload.
    pub parameters: Vec<u8>,
}

impl EzspFrame {
    /// Build a new outbound host→NCP command frame.
    #[must_use]
    pub fn command(sequence: u8, frame_id: u16, parameters: Vec<u8>) -> Self {
        Self {
            sequence,
            control: EzspFrameControl::command(),
            frame_id,
            parameters,
        }
    }

    /// Encode this frame to its on-wire byte form.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.parameters.len());
        out.push(self.sequence);
        let [lo, hi] = self.control.to_bytes();
        out.push(lo);
        out.push(hi);
        out.extend_from_slice(&self.frame_id.to_le_bytes());
        out.extend_from_slice(&self.parameters);
        out
    }

    /// Decode an on-wire frame.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] when the buffer is shorter than the header.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 5 {
            return Err(ZigbeeError::Truncated {
                need: 5,
                have: bytes.len(),
            });
        }
        let sequence = bytes[0];
        let control = EzspFrameControl::from_bytes(bytes[1], bytes[2]);
        let frame_id = u16::from_le_bytes([bytes[3], bytes[4]]);
        let parameters = bytes[5..].to_vec();
        Ok(Self {
            sequence,
            control,
            frame_id,
            parameters,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_frame_round_trip() {
        let f = EzspFrame::command(0x10, 0x0000, vec![]);
        let bytes = f.encode();
        let decoded = EzspFrame::decode(&bytes).unwrap();
        assert_eq!(decoded, f);
        // Header is 5 bytes.
        assert_eq!(bytes.len(), 5);
        assert_eq!(bytes[0], 0x10);
    }

    #[test]
    fn response_frame_round_trip() {
        let mut f = EzspFrame::command(0x42, 0x0017, vec![0xaa, 0xbb]);
        f.control = EzspFrameControl::response();
        let bytes = f.encode();
        let decoded = EzspFrame::decode(&bytes).unwrap();
        assert_eq!(decoded, f);
        assert!(decoded.control.response);
    }

    #[test]
    fn truncated_frame_rejected() {
        assert!(matches!(
            EzspFrame::decode(&[0x01, 0x02, 0x03]),
            Err(ZigbeeError::Truncated { .. })
        ));
    }

    #[test]
    fn frame_id_little_endian() {
        let f = EzspFrame::command(0x00, 0x1234, vec![]);
        let bytes = f.encode();
        // Frame ID at offset 3..5 is LE.
        assert_eq!(bytes[3], 0x34);
        assert_eq!(bytes[4], 0x12);
    }
}
