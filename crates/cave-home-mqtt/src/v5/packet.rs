//! MQTT 5.0 control-packet types (§3) — clean-room from OASIS 5.0.
//!
//! These mirror the 3.1.1 packets in [`crate::packet`] but add the
//! per-packet property block (§2.2.2) and replace return-code bytes
//! with §2.4 Reason Codes. The `QoS` type is shared with 3.1.1.

use crate::packet::QoS;
use crate::v5::property::Property;
use crate::v5::reason::ReasonCode;
use bytes::Bytes;

/// MQTT 5.0 §2.1.2 — control packet type, including the new AUTH (15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketTypeV5 {
    Connect = 1,
    ConnAck = 2,
    Publish = 3,
    PubAck = 4,
    PubRec = 5,
    PubRel = 6,
    PubComp = 7,
    Subscribe = 8,
    SubAck = 9,
    Unsubscribe = 10,
    UnsubAck = 11,
    PingReq = 12,
    PingResp = 13,
    Disconnect = 14,
    Auth = 15,
}

/// §3.1.3.2-3.1.3.4 — the Will message carried in a CONNECT payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Will {
    pub topic: String,
    pub payload: Bytes,
    pub qos: QoS,
    pub retain: bool,
    pub properties: Vec<Property>,
}

/// §3.1 CONNECT (protocol level 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectV5 {
    pub client_id: String,
    pub clean_start: bool,
    pub keep_alive_secs: u16,
    pub properties: Vec<Property>,
    pub will: Option<Will>,
    pub username: Option<String>,
    pub password: Option<Bytes>,
}

/// §3.2 CONNACK.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnAckV5 {
    pub session_present: bool,
    pub reason_code: ReasonCode,
    pub properties: Vec<Property>,
}

/// §3.3 PUBLISH. `packet_id` is `None` for QoS 0 (§3.3.2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishV5 {
    pub topic: String,
    pub qos: QoS,
    pub retain: bool,
    pub dup: bool,
    pub packet_id: Option<u16>,
    pub properties: Vec<Property>,
    pub payload: Bytes,
}

/// §3.4 PUBACK. The four QoS 1/2 acks (§3.4-§3.7) share one shape:
/// packet identifier + Reason Code + properties. A reason of Success
/// with no properties is encoded in the compact 2-byte form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PubAckV5 {
    pub packet_id: u16,
    pub reason_code: ReasonCode,
    pub properties: Vec<Property>,
}

/// §3.5 PUBREC — QoS 2 publish received (same shape as PUBACK).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PubRecV5 {
    pub packet_id: u16,
    pub reason_code: ReasonCode,
    pub properties: Vec<Property>,
}

/// §3.6 PUBREL — QoS 2 publish release (same shape as PUBACK).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PubRelV5 {
    pub packet_id: u16,
    pub reason_code: ReasonCode,
    pub properties: Vec<Property>,
}

/// §3.7 PUBCOMP — QoS 2 publish complete (same shape as PUBACK).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PubCompV5 {
    pub packet_id: u16,
    pub reason_code: ReasonCode,
    pub properties: Vec<Property>,
}

/// §3.8.3.3 — Retain Handling option in a subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RetainHandling {
    /// 0 — send retained messages at subscribe time.
    SendOnSubscribe = 0,
    /// 1 — send retained messages only if the subscription is new.
    SendIfNew = 1,
    /// 2 — do not send retained messages at subscribe time.
    DoNotSend = 2,
}

impl RetainHandling {
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::SendOnSubscribe,
            1 => Self::SendIfNew,
            2 => Self::DoNotSend,
            _ => return None,
        })
    }
}

/// §3.8.3 — one topic filter plus its §3.8.3.1 Subscription Options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionV5 {
    pub topic_filter: String,
    pub qos: QoS,
    pub no_local: bool,
    pub retain_as_published: bool,
    pub retain_handling: RetainHandling,
}

/// §3.8 SUBSCRIBE.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscribeV5 {
    pub packet_id: u16,
    pub properties: Vec<Property>,
    pub subscriptions: Vec<SubscriptionV5>,
}

/// §3.9 SUBACK.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubAckV5 {
    pub packet_id: u16,
    pub properties: Vec<Property>,
    pub reason_codes: Vec<ReasonCode>,
}

/// §3.10 UNSUBSCRIBE.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsubscribeV5 {
    pub packet_id: u16,
    pub properties: Vec<Property>,
    pub topic_filters: Vec<String>,
}

/// §3.11 UNSUBACK.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsubAckV5 {
    pub packet_id: u16,
    pub properties: Vec<Property>,
    pub reason_codes: Vec<ReasonCode>,
}

/// §3.14 DISCONNECT. Remaining Length 0 ⇒ Normal disconnection (§3.14.2.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisconnectV5 {
    pub reason_code: ReasonCode,
    pub properties: Vec<Property>,
}

/// §3.15 AUTH — extended/enhanced authentication exchange.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthV5 {
    pub reason_code: ReasonCode,
    pub properties: Vec<Property>,
}

/// A decoded/encodable MQTT 5.0 control packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PacketV5 {
    Connect(ConnectV5),
    ConnAck(ConnAckV5),
    Publish(PublishV5),
    PubAck(PubAckV5),
    PubRec(PubRecV5),
    PubRel(PubRelV5),
    PubComp(PubCompV5),
    Subscribe(SubscribeV5),
    SubAck(SubAckV5),
    Unsubscribe(UnsubscribeV5),
    UnsubAck(UnsubAckV5),
    PingReq,
    PingResp,
    Disconnect(DisconnectV5),
    Auth(AuthV5),
}

impl PacketV5 {
    pub fn packet_type(&self) -> PacketTypeV5 {
        match self {
            Self::Connect(_) => PacketTypeV5::Connect,
            Self::ConnAck(_) => PacketTypeV5::ConnAck,
            Self::Publish(_) => PacketTypeV5::Publish,
            Self::PubAck(_) => PacketTypeV5::PubAck,
            Self::PubRec(_) => PacketTypeV5::PubRec,
            Self::PubRel(_) => PacketTypeV5::PubRel,
            Self::PubComp(_) => PacketTypeV5::PubComp,
            Self::Subscribe(_) => PacketTypeV5::Subscribe,
            Self::SubAck(_) => PacketTypeV5::SubAck,
            Self::Unsubscribe(_) => PacketTypeV5::Unsubscribe,
            Self::UnsubAck(_) => PacketTypeV5::UnsubAck,
            Self::PingReq => PacketTypeV5::PingReq,
            Self::PingResp => PacketTypeV5::PingResp,
            Self::Disconnect(_) => PacketTypeV5::Disconnect,
            Self::Auth(_) => PacketTypeV5::Auth,
        }
    }
}
