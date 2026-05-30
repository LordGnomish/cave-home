//! MQTT 3.1.1 packet types — clean-room from OASIS MQTT 3.1.1 §2-§3.
//!
//! Only the Phase 1 trio (CONNECT, CONNACK, PUBLISH) is modelled.
//! Subscribe + Unsubscribe + Ping + Disconnect (§3.8-§3.14) land in
//! Phase 1b together with the session router.

use bytes::Bytes;

/// MQTT 3.1.1 §2.2.1 — control packet types as encoded in the upper
/// nibble of the fixed header's first byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    Connect     = 1,
    ConnAck     = 2,
    Publish     = 3,
    PubAck      = 4,
    PubRec      = 5,
    PubRel      = 6,
    PubComp     = 7,
    Subscribe   = 8,
    SubAck      = 9,
    Unsubscribe = 10,
    UnsubAck    = 11,
    PingReq     = 12,
    PingResp    = 13,
    Disconnect  = 14,
}

impl PacketType {
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::Connect,
            2 => Self::ConnAck,
            3 => Self::Publish,
            4 => Self::PubAck,
            5 => Self::PubRec,
            6 => Self::PubRel,
            7 => Self::PubComp,
            8 => Self::Subscribe,
            9 => Self::SubAck,
            10 => Self::Unsubscribe,
            11 => Self::UnsubAck,
            12 => Self::PingReq,
            13 => Self::PingResp,
            14 => Self::Disconnect,
            _ => return None,
        })
    }
}

/// MQTT 3.1.1 §4.3 — Quality of Service values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum QoS {
    AtMostOnce  = 0,
    AtLeastOnce = 1,
    ExactlyOnce = 2,
}

impl QoS {
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::AtMostOnce,
            1 => Self::AtLeastOnce,
            2 => Self::ExactlyOnce,
            _ => return None,
        })
    }
}

/// MQTT 3.1.1 §3.1 CONNECT — variable header + payload subset used
/// during Phase 1 (clean session + client_id + keep_alive). Will,
/// username/password, and per-connection auth land in Phase 1b.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connect {
    pub client_id: String,
    pub clean_session: bool,
    pub keep_alive_secs: u16,
}

/// MQTT 3.1.1 §3.2.2.3 — CONNACK return codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnAckReturnCode {
    Accepted                    = 0,
    UnacceptableProtocolVersion = 1,
    IdentifierRejected          = 2,
    ServerUnavailable           = 3,
    BadUsernameOrPassword       = 4,
    NotAuthorized               = 5,
}

impl ConnAckReturnCode {
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Accepted,
            1 => Self::UnacceptableProtocolVersion,
            2 => Self::IdentifierRejected,
            3 => Self::ServerUnavailable,
            4 => Self::BadUsernameOrPassword,
            5 => Self::NotAuthorized,
            _ => return None,
        })
    }
}

/// MQTT 3.1.1 §3.2 CONNACK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnAck {
    pub session_present: bool,
    pub return_code: ConnAckReturnCode,
}

/// MQTT 3.1.1 §3.3 PUBLISH. `packet_id` is `None` for QoS 0 (§3.3.2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Publish {
    pub topic: String,
    pub qos: QoS,
    pub retain: bool,
    pub dup: bool,
    pub packet_id: Option<u16>,
    pub payload: Bytes,
}

/// Top-level decoded/encoded MQTT packet (Phase 1 subset).
///
/// The three field-less variants are the zero-length control packets
/// §3.12 PINGREQ, §3.13 PINGRESP, and §3.14 DISCONNECT — each carries
/// no variable header or payload, only the two-byte fixed header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    Connect(Connect),
    ConnAck(ConnAck),
    Publish(Publish),
    PingReq,
    PingResp,
    Disconnect,
}

impl Packet {
    pub fn packet_type(&self) -> PacketType {
        match self {
            Self::Connect(_) => PacketType::Connect,
            Self::ConnAck(_) => PacketType::ConnAck,
            Self::Publish(_) => PacketType::Publish,
            Self::PingReq => PacketType::PingReq,
            Self::PingResp => PacketType::PingResp,
            Self::Disconnect => PacketType::Disconnect,
        }
    }
}
