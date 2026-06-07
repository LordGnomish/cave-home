//! MQTT 5.0 §2.2.2 — Properties.

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{Bytes, BytesMut};

    fn round_trip(props: &[Property]) -> Vec<Property> {
        let mut buf = BytesMut::new();
        encode_properties(props, &mut buf).expect("encode");
        let mut cursor: &[u8] = &buf;
        let decoded = decode_properties(&mut cursor).expect("decode");
        assert!(cursor.is_empty(), "decoder must consume the whole block");
        decoded
    }

    #[test]
    fn empty_property_set_is_a_single_zero_length_byte() {
        // §2.2.1.1: the Property Length is a Variable Byte Integer; an
        // empty set is encoded as the single byte 0x00.
        let mut buf = BytesMut::new();
        encode_properties(&[], &mut buf).expect("encode");
        assert_eq!(&buf[..], &[0x00]);
        assert_eq!(round_trip(&[]), Vec::<Property>::new());
    }

    #[test]
    fn mixed_property_set_round_trips_in_order() {
        // One of every wire data-type: byte, u32, utf8, binary, var-int,
        // and a repeated UTF-8 string pair (§2.2.2.2 User Property).
        let props = vec![
            Property::PayloadFormatIndicator(1),
            Property::MessageExpiryInterval(3600),
            Property::ContentType("application/json".into()),
            Property::ResponseTopic("home/reply".into()),
            Property::CorrelationData(Bytes::from_static(b"\x00\x01\xff")),
            Property::SubscriptionIdentifier(268_435_455),
            Property::UserProperty("region".into(), "loft".into()),
            Property::UserProperty("region".into(), "loft".into()),
        ];
        assert_eq!(round_trip(&props), props);
    }

    #[test]
    fn connect_scoped_properties_round_trip() {
        let props = vec![
            Property::SessionExpiryInterval(120),
            Property::ReceiveMaximum(20),
            Property::MaximumPacketSize(1 << 20),
            Property::TopicAliasMaximum(10),
            Property::RequestResponseInformation(1),
            Property::RequestProblemInformation(0),
            Property::AuthenticationMethod("SCRAM-SHA-256".into()),
            Property::AuthenticationData(Bytes::from_static(b"nonce")),
        ];
        assert_eq!(round_trip(&props), props);
    }

    #[test]
    fn length_prefix_is_a_variable_byte_integer() {
        // A reason string >127 bytes forces a 2-byte property-length prefix.
        let long = "x".repeat(300);
        let props = vec![Property::ReasonString(long.clone())];
        let mut buf = BytesMut::new();
        encode_properties(&props, &mut buf).expect("encode");
        // 0xAD 0x02 == 301 (300 payload + 1 id byte... 1 id + 2 strlen + 300).
        // Just assert it round-trips and the first length byte has the
        // continuation bit set (value > 127).
        assert!(buf[0] & 0x80 != 0, "multi-byte var-int length expected");
        assert_eq!(round_trip(&props), props);
    }

    #[test]
    fn decode_rejects_unknown_property_identifier() {
        // 0x07 is not an assigned property identifier in §2.2.2.2.
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0x02, 0x07, 0x00]); // len=2, id=0x07, ...
        let mut cursor: &[u8] = &buf;
        assert!(matches!(
            decode_properties(&mut cursor),
            Err(crate::v5::Error::UnknownProperty(0x07))
        ));
    }

    #[test]
    fn decode_rejects_truncated_property_block() {
        // Declares 10 bytes of properties but supplies none.
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0x0a]);
        let mut cursor: &[u8] = &buf;
        assert!(matches!(
            decode_properties(&mut cursor),
            Err(crate::v5::Error::Underflow { .. })
        ));
    }
}
