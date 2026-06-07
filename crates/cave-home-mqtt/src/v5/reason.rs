//! MQTT 5.0 §2.4 — Reason Codes.
//!
//! A Reason Code is a single byte indicating the result of an operation.
//! The same byte value carries a context-dependent name across packets
//! (e.g. `0x00` is *Success* in PUBACK, *Normal disconnection* in
//! DISCONNECT, and *Granted QoS 0* in SUBACK); we model the byte once
//! and document the aliases. Codes in `0x80..=0xFF` denote failure
//! (§2.4: "a Reason Code value of 0x80 or greater is an error").

/// The MQTT 5.0 §2.4 Reason Code table. Values are the on-wire bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ReasonCode {
    /// `0x00` — Success / Normal disconnection / Granted QoS 0.
    Success = 0x00,
    /// `0x01` — Granted QoS 1 (SUBACK).
    GrantedQoS1 = 0x01,
    /// `0x02` — Granted QoS 2 (SUBACK).
    GrantedQoS2 = 0x02,
    /// `0x04` — Disconnect with Will Message (DISCONNECT, client→server).
    DisconnectWithWill = 0x04,
    /// `0x10` — No matching subscribers (PUBACK/PUBREC).
    NoMatchingSubscribers = 0x10,
    /// `0x11` — No subscription existed (UNSUBACK).
    NoSubscriptionExisted = 0x11,
    /// `0x18` — Continue authentication (AUTH).
    ContinueAuthentication = 0x18,
    /// `0x19` — Re-authenticate (AUTH, client→server).
    ReAuthenticate = 0x19,
    UnspecifiedError = 0x80,
    MalformedPacket = 0x81,
    ProtocolError = 0x82,
    ImplementationSpecificError = 0x83,
    UnsupportedProtocolVersion = 0x84,
    ClientIdentifierNotValid = 0x85,
    BadUserNameOrPassword = 0x86,
    NotAuthorized = 0x87,
    ServerUnavailable = 0x88,
    ServerBusy = 0x89,
    Banned = 0x8A,
    ServerShuttingDown = 0x8B,
    BadAuthenticationMethod = 0x8C,
    KeepAliveTimeout = 0x8D,
    SessionTakenOver = 0x8E,
    TopicFilterInvalid = 0x8F,
    TopicNameInvalid = 0x90,
    PacketIdentifierInUse = 0x91,
    PacketIdentifierNotFound = 0x92,
    ReceiveMaximumExceeded = 0x93,
    TopicAliasInvalid = 0x94,
    PacketTooLarge = 0x95,
    MessageRateTooHigh = 0x96,
    QuotaExceeded = 0x97,
    AdministrativeAction = 0x98,
    PayloadFormatInvalid = 0x99,
    RetainNotSupported = 0x9A,
    QoSNotSupported = 0x9B,
    UseAnotherServer = 0x9C,
    ServerMoved = 0x9D,
    SharedSubscriptionsNotSupported = 0x9E,
    ConnectionRateExceeded = 0x9F,
    MaximumConnectTime = 0xA0,
    SubscriptionIdentifiersNotSupported = 0xA1,
    WildcardSubscriptionsNotSupported = 0xA2,
}

impl ReasonCode {
    /// Decode the on-wire byte. Returns `None` for unassigned values.
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0x00 => Self::Success,
            0x01 => Self::GrantedQoS1,
            0x02 => Self::GrantedQoS2,
            0x04 => Self::DisconnectWithWill,
            0x10 => Self::NoMatchingSubscribers,
            0x11 => Self::NoSubscriptionExisted,
            0x18 => Self::ContinueAuthentication,
            0x19 => Self::ReAuthenticate,
            0x80 => Self::UnspecifiedError,
            0x81 => Self::MalformedPacket,
            0x82 => Self::ProtocolError,
            0x83 => Self::ImplementationSpecificError,
            0x84 => Self::UnsupportedProtocolVersion,
            0x85 => Self::ClientIdentifierNotValid,
            0x86 => Self::BadUserNameOrPassword,
            0x87 => Self::NotAuthorized,
            0x88 => Self::ServerUnavailable,
            0x89 => Self::ServerBusy,
            0x8A => Self::Banned,
            0x8B => Self::ServerShuttingDown,
            0x8C => Self::BadAuthenticationMethod,
            0x8D => Self::KeepAliveTimeout,
            0x8E => Self::SessionTakenOver,
            0x8F => Self::TopicFilterInvalid,
            0x90 => Self::TopicNameInvalid,
            0x91 => Self::PacketIdentifierInUse,
            0x92 => Self::PacketIdentifierNotFound,
            0x93 => Self::ReceiveMaximumExceeded,
            0x94 => Self::TopicAliasInvalid,
            0x95 => Self::PacketTooLarge,
            0x96 => Self::MessageRateTooHigh,
            0x97 => Self::QuotaExceeded,
            0x98 => Self::AdministrativeAction,
            0x99 => Self::PayloadFormatInvalid,
            0x9A => Self::RetainNotSupported,
            0x9B => Self::QoSNotSupported,
            0x9C => Self::UseAnotherServer,
            0x9D => Self::ServerMoved,
            0x9E => Self::SharedSubscriptionsNotSupported,
            0x9F => Self::ConnectionRateExceeded,
            0xA0 => Self::MaximumConnectTime,
            0xA1 => Self::SubscriptionIdentifiersNotSupported,
            0xA2 => Self::WildcardSubscriptionsNotSupported,
            _ => return None,
        })
    }

    /// §2.4 — `0x80` or greater is an error.
    pub fn is_error(self) -> bool {
        (self as u8) >= 0x80
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_family_round_trips() {
        // §2.4: 0x00 is Success / Normal disconnection / Granted QoS 0.
        for code in [
            ReasonCode::Success,
            ReasonCode::GrantedQoS1,
            ReasonCode::GrantedQoS2,
            ReasonCode::DisconnectWithWill,
            ReasonCode::NoMatchingSubscribers,
        ] {
            assert_eq!(ReasonCode::from_u8(code as u8), Some(code));
            assert!(!code.is_error(), "{code:?} is a success code");
        }
    }

    #[test]
    fn error_family_round_trips_and_classifies() {
        // §2.4: codes >= 0x80 are failures.
        for code in [
            ReasonCode::UnspecifiedError,
            ReasonCode::MalformedPacket,
            ReasonCode::ProtocolError,
            ReasonCode::UnsupportedProtocolVersion,
            ReasonCode::ClientIdentifierNotValid,
            ReasonCode::BadUserNameOrPassword,
            ReasonCode::NotAuthorized,
            ReasonCode::TopicNameInvalid,
            ReasonCode::PacketTooLarge,
            ReasonCode::QuotaExceeded,
            ReasonCode::PayloadFormatInvalid,
            ReasonCode::SharedSubscriptionsNotSupported,
            ReasonCode::WildcardSubscriptionsNotSupported,
        ] {
            assert_eq!(ReasonCode::from_u8(code as u8), Some(code));
            assert!(code.is_error(), "{code:?} (0x{:02x}) is an error code", code as u8);
            assert!((code as u8) >= 0x80);
        }
    }

    #[test]
    fn unknown_reason_code_is_rejected() {
        // 0x03 is not assigned in the §2.4 table.
        assert_eq!(ReasonCode::from_u8(0x03), None);
        assert_eq!(ReasonCode::from_u8(0x7f), None);
    }
}
