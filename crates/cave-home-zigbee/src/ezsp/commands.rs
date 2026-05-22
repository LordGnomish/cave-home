// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! EZSP commands cave-home Phase 1 uses — Silicon Labs UG100 §4 (public).
//!
//! Frame IDs and parameter layouts here are derived from the public
//! Silicon Labs UG100 reference. Only the slice we actually need for
//! Phase 1 coordinator init is implemented; further commands land in
//! Phase 1b without breaking the [`EzspCommand`] enum (we add new
//! variants alongside).

use super::frame::EzspFrame;
use crate::error::{Result, ZigbeeError};

/// EZSP frame IDs used in Phase 1 (UG100 §4).
pub mod frame_id {
    /// 0x0000 — `version` request/response.
    pub const VERSION: u16 = 0x0000;
    /// 0x0017 — `networkInit` request.
    pub const NETWORK_INIT: u16 = 0x0017;
    /// 0x0022 — `permitJoining` request.
    pub const PERMIT_JOINING: u16 = 0x0022;
    /// 0x001e — `formNetwork` request.
    pub const FORM_NETWORK: u16 = 0x001e;
    /// 0x0025 — `getNetworkParameters` request.
    pub const GET_NETWORK_PARAMETERS: u16 = 0x0025;
    /// 0x0019 — `leaveNetwork` request.
    pub const LEAVE_NETWORK: u16 = 0x0019;
}

/// Coordinator network parameters returned by `getNetworkParameters`.
///
/// Only the fields cave-home uses are extracted. The full struct is
/// larger in UG100 but the trailing fields are not relevant at Phase 1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EzspNetworkParameters {
    /// 802.15.4 channel (11..=26 for the 2.4 GHz band).
    pub channel: u8,
    /// PAN ID.
    pub pan_id: u16,
    /// Extended (64-bit) PAN ID, little-endian on the wire.
    pub extended_pan_id: u64,
    /// Network update ID (counter the coordinator bumps after a channel switch).
    pub network_update_id: u8,
}

impl EzspNetworkParameters {
    /// Decode from the `getNetworkParameters` response payload.
    ///
    /// Layout per UG100: status (u8), node-type (u8), parameters
    /// (extended PAN id 8B, PAN id 2B, tx power 1B, channel 1B, join
    /// method 1B, manager 2B, channels mask 4B, update id 1B).
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] if the buffer is shorter
    /// than the documented response layout.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        // We need at least 1 status + 1 nodeType + (8 + 2 + 1 + 1) = 13 bytes.
        if bytes.len() < 22 {
            return Err(ZigbeeError::Truncated {
                need: 22,
                have: bytes.len(),
            });
        }
        let status = bytes[0];
        if status != 0x00 {
            return Err(ZigbeeError::Ezsp(format!(
                "getNetworkParameters status 0x{status:02x}"
            )));
        }
        // node_type byte = bytes[1].
        let ext_pan_bytes: [u8; 8] = bytes[2..10].try_into().expect("bounds checked");
        let extended_pan_id = u64::from_le_bytes(ext_pan_bytes);
        let pan_id = u16::from_le_bytes([bytes[10], bytes[11]]);
        // tx_power = bytes[12]
        let channel = bytes[13];
        // join_method = bytes[14], manager id = bytes[15..17],
        // channels mask = bytes[17..21]
        let network_update_id = bytes[21];
        Ok(Self {
            channel,
            pan_id,
            extended_pan_id,
            network_update_id,
        })
    }
}

/// One EZSP command (host→NCP).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EzspCommand {
    /// `version(desiredProtocolVersion)` — UG100 §4.1.
    Version { desired_protocol_version: u8 },
    /// `networkInit()` — UG100 §4.5.
    NetworkInit,
    /// `permitJoining(duration)` — UG100 §4.10.
    PermitJoining { duration_seconds: u8 },
    /// `formNetwork(params)` — UG100 §4.7 (params are pre-encoded so
    /// the higher layer can tune them per coordinator family).
    FormNetwork { encoded_params: Vec<u8> },
    /// `getNetworkParameters()` — UG100 §4.13.
    GetNetworkParameters,
    /// `leaveNetwork()` — UG100 §4.16.
    LeaveNetwork,
}

impl EzspCommand {
    /// Frame ID for this command.
    #[must_use]
    pub const fn frame_id(&self) -> u16 {
        match self {
            Self::Version { .. } => frame_id::VERSION,
            Self::NetworkInit => frame_id::NETWORK_INIT,
            Self::PermitJoining { .. } => frame_id::PERMIT_JOINING,
            Self::FormNetwork { .. } => frame_id::FORM_NETWORK,
            Self::GetNetworkParameters => frame_id::GET_NETWORK_PARAMETERS,
            Self::LeaveNetwork => frame_id::LEAVE_NETWORK,
        }
    }

    /// Encode the parameter payload (excludes the EZSP header).
    #[must_use]
    pub fn encode_parameters(&self) -> Vec<u8> {
        match self {
            Self::Version {
                desired_protocol_version,
            } => vec![*desired_protocol_version],
            Self::NetworkInit
            | Self::GetNetworkParameters
            | Self::LeaveNetwork => Vec::new(),
            Self::PermitJoining { duration_seconds } => vec![*duration_seconds],
            Self::FormNetwork { encoded_params } => encoded_params.clone(),
        }
    }

    /// Build the full EZSP frame for this command with sequence `sequence`.
    #[must_use]
    pub fn to_frame(&self, sequence: u8) -> EzspFrame {
        EzspFrame::command(sequence, self.frame_id(), self.encode_parameters())
    }
}

/// Decoded EZSP response — one variant per command we issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EzspResponse {
    /// `version` response: (protocol_version, stack_type, stack_version).
    Version {
        protocol_version: u8,
        stack_type: u8,
        stack_version: u16,
    },
    /// `networkInit` status (0x00 == success).
    NetworkInitStatus(u8),
    /// `permitJoining` status.
    PermitJoiningStatus(u8),
    /// `formNetwork` status.
    FormNetworkStatus(u8),
    /// `getNetworkParameters` parsed parameters.
    GetNetworkParameters(EzspNetworkParameters),
    /// `leaveNetwork` status.
    LeaveNetworkStatus(u8),
    /// An EZSP frame whose ID we don't know yet at Phase 1 (kept opaque).
    Unknown { frame_id: u16, parameters: Vec<u8> },
}

impl EzspResponse {
    /// Build a typed response from a raw incoming EZSP frame.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] if the parameter payload is
    /// shorter than the documented response layout.
    pub fn from_frame(frame: &EzspFrame) -> Result<Self> {
        let p = frame.parameters.as_slice();
        Ok(match frame.frame_id {
            frame_id::VERSION => {
                if p.len() < 4 {
                    return Err(ZigbeeError::Truncated {
                        need: 4,
                        have: p.len(),
                    });
                }
                Self::Version {
                    protocol_version: p[0],
                    stack_type: p[1],
                    stack_version: u16::from_le_bytes([p[2], p[3]]),
                }
            }
            frame_id::NETWORK_INIT => {
                if p.is_empty() {
                    return Err(ZigbeeError::Truncated { need: 1, have: 0 });
                }
                Self::NetworkInitStatus(p[0])
            }
            frame_id::PERMIT_JOINING => {
                if p.is_empty() {
                    return Err(ZigbeeError::Truncated { need: 1, have: 0 });
                }
                Self::PermitJoiningStatus(p[0])
            }
            frame_id::FORM_NETWORK => {
                if p.is_empty() {
                    return Err(ZigbeeError::Truncated { need: 1, have: 0 });
                }
                Self::FormNetworkStatus(p[0])
            }
            frame_id::GET_NETWORK_PARAMETERS => {
                Self::GetNetworkParameters(EzspNetworkParameters::decode(p)?)
            }
            frame_id::LEAVE_NETWORK => {
                if p.is_empty() {
                    return Err(ZigbeeError::Truncated { need: 1, have: 0 });
                }
                Self::LeaveNetworkStatus(p[0])
            }
            id => Self::Unknown {
                frame_id: id,
                parameters: p.to_vec(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_command_encodes_correctly() {
        let cmd = EzspCommand::Version {
            desired_protocol_version: 8,
        };
        assert_eq!(cmd.frame_id(), frame_id::VERSION);
        assert_eq!(cmd.encode_parameters(), vec![8]);
        let frame = cmd.to_frame(0x01);
        let bytes = frame.encode();
        // sequence + 2 fc + 2 frame id + 1 param = 6 bytes.
        assert_eq!(bytes.len(), 6);
    }

    #[test]
    fn permit_joining_carries_duration() {
        let cmd = EzspCommand::PermitJoining {
            duration_seconds: 254,
        };
        assert_eq!(cmd.encode_parameters(), vec![254]);
    }

    #[test]
    fn response_parses_version_layout() {
        let frame = EzspFrame {
            sequence: 0,
            control: super::super::frame::EzspFrameControl::response(),
            frame_id: frame_id::VERSION,
            parameters: vec![0x08, 0x02, 0x34, 0x12],
        };
        let resp = EzspResponse::from_frame(&frame).unwrap();
        assert_eq!(
            resp,
            EzspResponse::Version {
                protocol_version: 0x08,
                stack_type: 0x02,
                stack_version: 0x1234,
            }
        );
    }

    #[test]
    fn unknown_frame_id_returned_opaquely() {
        let frame = EzspFrame::command(0, 0xbeef, vec![0xde, 0xad]);
        let resp = EzspResponse::from_frame(&frame).unwrap();
        assert_eq!(
            resp,
            EzspResponse::Unknown {
                frame_id: 0xbeef,
                parameters: vec![0xde, 0xad],
            }
        );
    }

    #[test]
    fn network_parameters_round_trip() {
        // status 00, node_type 01, extended pan id 0x0123456789abcdef LE,
        // pan id 0x1a2b, tx_power 8, channel 15, join_method 0,
        // manager id 0x0000, channels mask 0x07fff800, update id 7.
        let buf = [
            0x00, 0x01, 0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01, 0x2b, 0x1a, 0x08, 0x0f,
            0x00, 0x00, 0x00, 0x00, 0xf8, 0xff, 0x07, 0x07,
        ];
        let params = EzspNetworkParameters::decode(&buf).unwrap();
        assert_eq!(params.channel, 15);
        assert_eq!(params.pan_id, 0x1a2b);
        assert_eq!(params.extended_pan_id, 0x0123_4567_89ab_cdef);
        assert_eq!(params.network_update_id, 7);
    }

    #[test]
    fn network_parameters_truncated_rejected() {
        assert!(matches!(
            EzspNetworkParameters::decode(&[0x00, 0x01]),
            Err(ZigbeeError::Truncated { .. })
        ));
    }

    #[test]
    fn network_parameters_non_success_rejected() {
        let mut buf = [0u8; 22];
        buf[0] = 0x02; // status != 0
        assert!(matches!(
            EzspNetworkParameters::decode(&buf),
            Err(ZigbeeError::Ezsp(_))
        ));
    }
}
