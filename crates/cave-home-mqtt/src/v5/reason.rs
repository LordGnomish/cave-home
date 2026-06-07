//! MQTT 5.0 §2.4 — Reason Codes.

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
