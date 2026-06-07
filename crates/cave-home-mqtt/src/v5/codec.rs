//! MQTT 5.0 control-packet wire codec (§3) — clean-room from OASIS 5.0.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::QoS;
    use crate::v5::packet::*;
    use crate::v5::property::Property;
    use crate::v5::reason::ReasonCode;
    use bytes::Bytes;

    fn round_trip(p: &PacketV5) {
        let bytes = encode_v5(p).expect("encode");
        let (back, used) = decode_v5(&bytes).expect("decode");
        assert_eq!(used, bytes.len(), "decoder must consume the whole frame");
        assert_eq!(&back, p);
    }

    #[test]
    fn connect_v5_full_round_trip() {
        let c = ConnectV5 {
            client_id: "loft-pi".into(),
            clean_start: true,
            keep_alive_secs: 30,
            properties: vec![
                Property::SessionExpiryInterval(3600),
                Property::ReceiveMaximum(50),
            ],
            will: Some(Will {
                topic: "home/loft/status".into(),
                payload: Bytes::from_static(b"offline"),
                qos: QoS::AtLeastOnce,
                retain: true,
                properties: vec![Property::WillDelayInterval(10)],
            }),
            username: Some("admin".into()),
            password: Some(Bytes::from_static(b"s3cret")),
        };
        round_trip(&PacketV5::Connect(c));
    }

    #[test]
    fn connect_v5_minimal_round_trip() {
        let c = ConnectV5 {
            client_id: String::new(),
            clean_start: true,
            keep_alive_secs: 0,
            properties: vec![],
            will: None,
            username: None,
            password: None,
        };
        round_trip(&PacketV5::Connect(c));
    }

    #[test]
    fn connect_v5_rejects_protocol_level_4() {
        // A v5 frame must declare protocol level 5 (§3.1.2.2).
        let c = ConnectV5 {
            client_id: "x".into(),
            clean_start: false,
            keep_alive_secs: 1,
            properties: vec![],
            will: None,
            username: None,
            password: None,
        };
        let mut bytes = encode_v5(&PacketV5::Connect(c)).unwrap();
        // Locate the protocol level byte: header(1) + remlen(1) + "MQTT"(6) = idx 8.
        bytes[8] = 4;
        assert!(matches!(decode_v5(&bytes), Err(crate::v5::Error::BadProtocolLevel(4))));
    }

    #[test]
    fn connack_v5_round_trip() {
        let c = ConnAckV5 {
            session_present: true,
            reason_code: ReasonCode::Success,
            properties: vec![
                Property::AssignedClientIdentifier("auto-123".into()),
                Property::MaximumQoS(1),
            ],
        };
        round_trip(&PacketV5::ConnAck(c));
    }

    #[test]
    fn publish_v5_round_trip_with_properties() {
        let p = PublishV5 {
            topic: "home/sensor/temp".into(),
            qos: QoS::AtLeastOnce,
            retain: false,
            dup: false,
            packet_id: Some(7),
            properties: vec![
                Property::PayloadFormatIndicator(1),
                Property::ContentType("text/plain".into()),
                Property::TopicAlias(4),
            ],
            payload: Bytes::from_static(b"21.7"),
        };
        round_trip(&PacketV5::Publish(p));
    }

    #[test]
    fn publish_v5_qos0_has_no_packet_id() {
        let p = PublishV5 {
            topic: "home/light".into(),
            qos: QoS::AtMostOnce,
            retain: true,
            dup: false,
            packet_id: None,
            properties: vec![],
            payload: Bytes::from_static(b"ON"),
        };
        round_trip(&PacketV5::Publish(p));
    }

    #[test]
    fn puback_v5_short_form_is_two_bytes() {
        // §3.4.2.1: a PUBACK with reason Success and no properties may
        // omit the reason code and property length (Remaining Length 2).
        let ack = PubAckV5 {
            packet_id: 42,
            reason_code: ReasonCode::Success,
            properties: vec![],
        };
        let bytes = encode_v5(&PacketV5::PubAck(ack.clone())).unwrap();
        assert_eq!(bytes.len(), 4, "fixed header(2) + packet id(2)");
        let (back, _) = decode_v5(&bytes).unwrap();
        assert_eq!(back, PacketV5::PubAck(ack));
    }

    #[test]
    fn puback_v5_long_form_carries_reason_and_props() {
        let ack = PubAckV5 {
            packet_id: 42,
            reason_code: ReasonCode::NoMatchingSubscribers,
            properties: vec![Property::ReasonString("nobody home".into())],
        };
        round_trip(&PacketV5::PubAck(ack));
    }

    #[test]
    fn pubrec_pubrel_pubcomp_v5_round_trip() {
        round_trip(&PacketV5::PubRec(PubRecV5 {
            packet_id: 9,
            reason_code: ReasonCode::Success,
            properties: vec![],
        }));
        round_trip(&PacketV5::PubRel(PubRelV5 {
            packet_id: 9,
            reason_code: ReasonCode::PacketIdentifierNotFound,
            properties: vec![],
        }));
        round_trip(&PacketV5::PubComp(PubCompV5 {
            packet_id: 9,
            reason_code: ReasonCode::Success,
            properties: vec![],
        }));
    }

    #[test]
    fn subscribe_v5_round_trip_with_options() {
        let s = SubscribeV5 {
            packet_id: 11,
            properties: vec![Property::SubscriptionIdentifier(5)],
            subscriptions: vec![
                SubscriptionV5 {
                    topic_filter: "home/+/temp".into(),
                    qos: QoS::ExactlyOnce,
                    no_local: true,
                    retain_as_published: true,
                    retain_handling: RetainHandling::SendIfNew,
                },
                SubscriptionV5 {
                    topic_filter: "#".into(),
                    qos: QoS::AtMostOnce,
                    no_local: false,
                    retain_as_published: false,
                    retain_handling: RetainHandling::DoNotSend,
                },
            ],
        };
        round_trip(&PacketV5::Subscribe(s));
    }

    #[test]
    fn subscribe_v5_rejects_reserved_subscription_option_bits() {
        // §3.8.3.1: bits 6-7 of the subscription options byte are reserved.
        let s = SubscribeV5 {
            packet_id: 1,
            properties: vec![],
            subscriptions: vec![SubscriptionV5 {
                topic_filter: "a".into(),
                qos: QoS::AtMostOnce,
                no_local: false,
                retain_as_published: false,
                retain_handling: RetainHandling::SendOnSubscribe,
            }],
        };
        let mut bytes = encode_v5(&PacketV5::Subscribe(s)).unwrap();
        // Flip a reserved high bit in the last byte (the options byte).
        let last = bytes.len() - 1;
        bytes[last] |= 0x40;
        assert!(matches!(decode_v5(&bytes), Err(crate::v5::Error::Malformed(_))));
    }

    #[test]
    fn suback_v5_round_trip() {
        let s = SubAckV5 {
            packet_id: 11,
            properties: vec![],
            reason_codes: vec![
                ReasonCode::GrantedQoS2,
                ReasonCode::Success, // 0x00 == Granted QoS 0
                ReasonCode::TopicFilterInvalid,
            ],
        };
        round_trip(&PacketV5::SubAck(s));
    }

    #[test]
    fn unsubscribe_and_unsuback_v5_round_trip() {
        round_trip(&PacketV5::Unsubscribe(UnsubscribeV5 {
            packet_id: 21,
            properties: vec![Property::UserProperty("x".into(), "y".into())],
            topic_filters: vec!["home/+/temp".into(), "#".into()],
        }));
        round_trip(&PacketV5::UnsubAck(UnsubAckV5 {
            packet_id: 21,
            properties: vec![],
            reason_codes: vec![ReasonCode::Success, ReasonCode::NoSubscriptionExisted],
        }));
    }

    #[test]
    fn ping_round_trip() {
        round_trip(&PacketV5::PingReq);
        round_trip(&PacketV5::PingResp);
    }

    #[test]
    fn disconnect_v5_short_and_long_form() {
        // §3.14.2.1: Remaining Length 0 ⇒ reason Normal disconnection (0x00).
        let short = DisconnectV5 { reason_code: ReasonCode::Success, properties: vec![] };
        let bytes = encode_v5(&PacketV5::Disconnect(short.clone())).unwrap();
        assert_eq!(bytes.len(), 2, "header byte + zero remaining length");
        let (back, _) = decode_v5(&bytes).unwrap();
        assert_eq!(back, PacketV5::Disconnect(short));

        round_trip(&PacketV5::Disconnect(DisconnectV5 {
            reason_code: ReasonCode::SessionTakenOver,
            properties: vec![Property::ReasonString("elsewhere".into())],
        }));
    }

    #[test]
    fn auth_v5_round_trip() {
        round_trip(&PacketV5::Auth(AuthV5 {
            reason_code: ReasonCode::ContinueAuthentication,
            properties: vec![
                Property::AuthenticationMethod("SCRAM-SHA-256".into()),
                Property::AuthenticationData(Bytes::from_static(b"server-first")),
            ],
        }));
    }

    #[test]
    fn decode_v5_reports_underflow_on_partial_frame() {
        let full = encode_v5(&PacketV5::PingReq).unwrap();
        // PINGREQ is 2 bytes; one byte must be insufficient.
        assert!(matches!(decode_v5(&full[..1]), Err(crate::v5::Error::Underflow { .. })));
    }
}
