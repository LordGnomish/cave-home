// SPDX-License-Identifier: Apache-2.0
//! Z-Wave Serial API constants.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/serial/src/message/MessageHeaders.ts
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/serial/src/message/Constants.ts
//!
//! The byte values here are normative — they are the wire-level encoding the
//! 500/700/800-series controllers expect. They must match upstream exactly;
//! see the `serial_constants_match_upstream` test below.

/// Single-byte signalling headers that frame the Z-Wave Serial API.
///
/// Upstream: `MessageHeaders` enum.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MessageHeader {
    /// Start of Frame.
    Sof = 0x01,
    /// Acknowledgement.
    Ack = 0x06,
    /// Negative acknowledgement.
    Nak = 0x15,
    /// Cancel — host should resend.
    Can = 0x18,
}

impl MessageHeader {
    /// Recognise a single-byte signalling header. Returns `None` for SOF
    /// (the SOF marker is the *start* of a data frame, not a complete
    /// signalling byte) and for unknown bytes.
    #[must_use]
    pub const fn from_signal_byte(b: u8) -> Option<Self> {
        match b {
            0x06 => Some(Self::Ack),
            0x15 => Some(Self::Nak),
            0x18 => Some(Self::Can),
            _ => None,
        }
    }

    /// Convert to its raw wire byte.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Whether a data frame is a request or a response.
///
/// Upstream: `MessageType` enum.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MessageType {
    /// Host -> module, or module-initiated request.
    Request = 0x00,
    /// Module's response to an earlier request.
    Response = 0x01,
}

impl MessageType {
    /// Parse from the wire byte.
    ///
    /// # Errors
    /// Returns `None` if the byte is not 0 or 1.
    #[must_use]
    pub const fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Request),
            0x01 => Some(Self::Response),
            _ => None,
        }
    }
}

/// Subset of `FunctionType` (Serial API function IDs) the Phase 1 driver
/// actually emits or recognises. The wire byte for each value matches
/// upstream exactly. Long-tail function types remain in [`FunctionType::Other`].
///
/// Upstream: `FunctionType` enum.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FunctionType {
    /// `GetSerialApiCapabilities` — controller fingerprint after boot.
    GetSerialApiCapabilities,
    /// `GetControllerId` — Home ID + own node ID.
    GetControllerId,
    /// `GetControllerVersion` — firmware version string.
    GetControllerVersion,
    /// `GetSerialApiInitData` — node list + Z-Wave LR capability bits.
    GetSerialApiInitData,
    /// `GetControllerCapabilities`.
    GetControllerCapabilities,
    /// `GetNodeProtocolInfo` — protocol info for a single node.
    GetNodeProtocolInfo,
    /// `SendData` — send a CC payload to a node.
    SendData,
    /// `RequestNodeInfo`.
    RequestNodeInfo,
    /// `ApplicationCommand` — module pushing an inbound CC payload to host.
    ApplicationCommand,
    /// `AddNodeToNetwork` — inclusion controller command.
    AddNodeToNetwork,
    /// `RemoveNodeFromNetwork` — exclusion controller command.
    RemoveNodeFromNetwork,
    /// `RequestNodeNeighborUpdate` — network heal.
    RequestNodeNeighborUpdate,
    /// `SoftReset` — module reset without losing network state.
    SoftReset,
    /// Catch-all for function bytes that Phase 1 does not yet interpret.
    Other(u8),
}

impl FunctionType {
    /// Decode from the wire byte.
    #[must_use]
    pub const fn from_u8(b: u8) -> Self {
        match b {
            0x07 => Self::GetSerialApiCapabilities,
            0x20 => Self::GetControllerId,
            0x15 => Self::GetControllerVersion,
            0x02 => Self::GetSerialApiInitData,
            0x05 => Self::GetControllerCapabilities,
            0x41 => Self::GetNodeProtocolInfo,
            0x13 => Self::SendData,
            0x60 => Self::RequestNodeInfo,
            0x04 => Self::ApplicationCommand,
            0x4a => Self::AddNodeToNetwork,
            0x4b => Self::RemoveNodeFromNetwork,
            0x48 => Self::RequestNodeNeighborUpdate,
            0x08 => Self::SoftReset,
            other => Self::Other(other),
        }
    }

    /// Encode to the wire byte.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::GetSerialApiCapabilities => 0x07,
            Self::GetControllerId => 0x20,
            Self::GetControllerVersion => 0x15,
            Self::GetSerialApiInitData => 0x02,
            Self::GetControllerCapabilities => 0x05,
            Self::GetNodeProtocolInfo => 0x41,
            Self::SendData => 0x13,
            Self::RequestNodeInfo => 0x60,
            Self::ApplicationCommand => 0x04,
            Self::AddNodeToNetwork => 0x4a,
            Self::RemoveNodeFromNetwork => 0x4b,
            Self::RequestNodeNeighborUpdate => 0x48,
            Self::SoftReset => 0x08,
            Self::Other(b) => b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locks the wire bytes against accidental edits — these are normative.
    /// See upstream `MessageHeaders.ts` and `Constants.ts`.
    #[test]
    fn serial_constants_match_upstream() {
        assert_eq!(MessageHeader::Sof.as_u8(), 0x01);
        assert_eq!(MessageHeader::Ack.as_u8(), 0x06);
        assert_eq!(MessageHeader::Nak.as_u8(), 0x15);
        assert_eq!(MessageHeader::Can.as_u8(), 0x18);

        assert_eq!(MessageType::Request as u8, 0x00);
        assert_eq!(MessageType::Response as u8, 0x01);

        assert_eq!(FunctionType::GetSerialApiCapabilities.as_u8(), 0x07);
        assert_eq!(FunctionType::GetControllerId.as_u8(), 0x20);
        assert_eq!(FunctionType::GetControllerVersion.as_u8(), 0x15);
        assert_eq!(FunctionType::GetSerialApiInitData.as_u8(), 0x02);
        assert_eq!(FunctionType::GetControllerCapabilities.as_u8(), 0x05);
        assert_eq!(FunctionType::GetNodeProtocolInfo.as_u8(), 0x41);
        assert_eq!(FunctionType::SendData.as_u8(), 0x13);
        assert_eq!(FunctionType::RequestNodeInfo.as_u8(), 0x60);
        assert_eq!(FunctionType::ApplicationCommand.as_u8(), 0x04);
        assert_eq!(FunctionType::AddNodeToNetwork.as_u8(), 0x4a);
        assert_eq!(FunctionType::RemoveNodeFromNetwork.as_u8(), 0x4b);
        assert_eq!(FunctionType::RequestNodeNeighborUpdate.as_u8(), 0x48);
        assert_eq!(FunctionType::SoftReset.as_u8(), 0x08);
    }

    #[test]
    fn function_type_round_trip() {
        for b in 0u8..=255 {
            let ft = FunctionType::from_u8(b);
            assert_eq!(ft.as_u8(), b, "round-trip failed for byte 0x{b:02x}");
        }
    }
}
