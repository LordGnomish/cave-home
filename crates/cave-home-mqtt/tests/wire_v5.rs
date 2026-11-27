//! Pinning tests for the MQTT 5.0 wire codec.
//!
//! Every frame below is written out by hand from the OASIS MQTT 5.0
//! specification (§1.5 data representation, §2.1 fixed header, §2.2.2
//! properties, §2.4 reason codes, §3.1-§3.15 control packets) and compared
//! byte for byte, so a refactor of the codec cannot change what goes on the
//! wire or which error a malformed frame produces.

use bytes::{Bytes, BytesMut};
use cave_home_mqtt::codec::CodecError;
use cave_home_mqtt::packet::QoS;
use cave_home_mqtt::v5::packet::{
    AuthV5, ConnAckV5, ConnectV5, DisconnectV5, PacketTypeV5, PubAckV5, PubCompV5, PubRecV5,
    PubRelV5, PublishV5, RetainHandling, SubAckV5, SubscribeV5, SubscriptionV5, UnsubAckV5,
    UnsubscribeV5, Will,
};
use cave_home_mqtt::v5::{
    Error, PacketV5, Property, ReasonCode, decode_properties, decode_v5, encode_properties,
    encode_v5,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const RESERVED_FLAGS: Error = Error::Malformed("reserved fixed-header flags must be 0b0010");

/// `packet` must encode to exactly `frame`, and `frame` must decode back to
/// `packet`, consuming every byte.
fn assert_frame(packet: &PacketV5, frame: &[u8]) -> TestResult {
    let encoded = encode_v5(packet)?;
    assert_eq!(&encoded[..], frame, "encoding of {packet:?}");
    let (decoded, used) = decode_v5(frame)?;
    assert_eq!(&decoded, packet, "decoding of {frame:02x?}");
    assert_eq!(used, frame.len(), "bytes consumed from {frame:02x?}");
    Ok(())
}

/// Each `(frame, error)` pair must fail to decode with exactly that error.
fn assert_rejected(cases: &[(&[u8], Error)]) {
    for (frame, expected) in cases {
        assert_eq!(
            decode_v5(frame).err().as_ref(),
            Some(expected),
            "frame {frame:02x?}"
        );
    }
}

/// Each `(block, error)` pair must fail to decode as a property block.
fn assert_properties_rejected(cases: &[(&[u8], Error)]) {
    for (block, expected) in cases {
        let mut cursor: &[u8] = block;
        assert_eq!(
            decode_properties(&mut cursor).err().as_ref(),
            Some(expected),
            "block {block:02x?}"
        );
    }
}

fn encoded_properties(props: &[Property]) -> Result<Vec<u8>, Error> {
    let mut out = BytesMut::new();
    encode_properties(props, &mut out)?;
    Ok(out.to_vec())
}

fn connect(client_id: &str) -> ConnectV5 {
    ConnectV5 {
        client_id: client_id.to_owned(),
        clean_start: false,
        keep_alive_secs: 0,
        properties: vec![],
        will: None,
        username: None,
        password: None,
    }
}

fn will(qos: QoS, retain: bool) -> Will {
    Will {
        topic: "w/t".to_owned(),
        payload: Bytes::from_static(b"bye"),
        qos,
        retain,
        properties: vec![],
    }
}

fn publish(qos: QoS, flags: (bool, bool), packet_id: Option<u16>) -> PublishV5 {
    let (dup, retain) = flags;
    PublishV5 {
        topic: "t".to_owned(),
        qos,
        retain,
        dup,
        packet_id,
        properties: vec![],
        payload: Bytes::new(),
    }
}

fn subscription(filter: &str, qos: QoS, options: (bool, bool, RetainHandling)) -> SubscriptionV5 {
    let (no_local, retain_as_published, retain_handling) = options;
    SubscriptionV5 {
        topic_filter: filter.to_owned(),
        qos,
        no_local,
        retain_as_published,
        retain_handling,
    }
}

// ---- §2.2.2 properties ----------------------------------------------------------

/// One property per identifier of §2.2.2.2 with its `identifier || value`
/// bytes.
fn property_table() -> Vec<(Property, Vec<u8>)> {
    let text = |s: &str| s.to_owned();
    vec![
        (Property::PayloadFormatIndicator(1), vec![0x01, 0x01]),
        (
            Property::MessageExpiryInterval(0x0102_0304),
            vec![0x02, 0x01, 0x02, 0x03, 0x04],
        ),
        (
            Property::ContentType(text("ab")),
            vec![0x03, 0x00, 0x02, b'a', b'b'],
        ),
        (
            Property::ResponseTopic(text("r/t")),
            vec![0x08, 0x00, 0x03, b'r', b'/', b't'],
        ),
        (
            Property::CorrelationData(Bytes::from_static(&[0x00, 0x01, 0xff])),
            vec![0x09, 0x00, 0x03, 0x00, 0x01, 0xff],
        ),
        (
            Property::SubscriptionIdentifier(268_435_455),
            vec![0x0b, 0xff, 0xff, 0xff, 0x7f],
        ),
        (
            Property::SessionExpiryInterval(u32::MAX),
            vec![0x11, 0xff, 0xff, 0xff, 0xff],
        ),
        (
            Property::AssignedClientIdentifier(text("auto-1")),
            vec![0x12, 0x00, 0x06, b'a', b'u', b't', b'o', b'-', b'1'],
        ),
        (Property::ServerKeepAlive(0x1234), vec![0x13, 0x12, 0x34]),
        (
            Property::AuthenticationMethod(text("m")),
            vec![0x15, 0x00, 0x01, b'm'],
        ),
        (
            Property::AuthenticationData(Bytes::from_static(&[0xde, 0xad])),
            vec![0x16, 0x00, 0x02, 0xde, 0xad],
        ),
        (Property::RequestProblemInformation(0), vec![0x17, 0x00]),
        (
            Property::WillDelayInterval(10),
            vec![0x18, 0x00, 0x00, 0x00, 0x0a],
        ),
        (Property::RequestResponseInformation(1), vec![0x19, 0x01]),
        (
            Property::ResponseInformation(text("i")),
            vec![0x1a, 0x00, 0x01, b'i'],
        ),
        (
            Property::ServerReference(text("s")),
            vec![0x1c, 0x00, 0x01, b's'],
        ),
        (
            Property::ReasonString(text("why")),
            vec![0x1f, 0x00, 0x03, b'w', b'h', b'y'],
        ),
        (Property::ReceiveMaximum(20), vec![0x21, 0x00, 0x14]),
        (Property::TopicAliasMaximum(10), vec![0x22, 0x00, 0x0a]),
        (Property::TopicAlias(4), vec![0x23, 0x00, 0x04]),
        (Property::MaximumQoS(1), vec![0x24, 0x01]),
        (Property::RetainAvailable(0), vec![0x25, 0x00]),
        (
            Property::UserProperty(text("k"), text("v")),
            vec![0x26, 0x00, 0x01, b'k', 0x00, 0x01, b'v'],
        ),
        (
            Property::MaximumPacketSize(1 << 20),
            vec![0x27, 0x00, 0x10, 0x00, 0x00],
        ),
        (Property::WildcardSubscriptionAvailable(1), vec![0x28, 0x01]),
        (
            Property::SubscriptionIdentifierAvailable(1),
            vec![0x29, 0x01],
        ),
        (Property::SharedSubscriptionAvailable(0), vec![0x2a, 0x00]),
    ]
}

#[test]
fn every_property_is_byte_exact_on_its_own() -> TestResult {
    let table = property_table();
    assert_eq!(table.len(), 27);
    for (property, bytes) in &table {
        assert_eq!(Some(&property.identifier()), bytes.first(), "{property:?}");
        let mut block = vec![u8::try_from(bytes.len())?];
        block.extend_from_slice(bytes);
        assert_eq!(
            encoded_properties(std::slice::from_ref(property))?,
            block,
            "{property:?}"
        );
        let mut cursor: &[u8] = &block;
        assert_eq!(
            decode_properties(&mut cursor)?,
            std::slice::from_ref(property),
            "{block:02x?}"
        );
        assert_eq!(cursor, [0_u8; 0], "{block:02x?}");
    }
    Ok(())
}

#[test]
fn a_block_with_every_property_keeps_wire_order() -> TestResult {
    let table = property_table();
    let props: Vec<Property> = table.iter().map(|(p, _)| p.clone()).collect();
    // 109 value bytes behind a one-byte Property Length.
    let mut block = vec![0x6d];
    for (_, bytes) in &table {
        block.extend_from_slice(bytes);
    }
    assert_eq!(block.len(), 110);
    assert_eq!(encoded_properties(&props)?, block);
    let mut cursor: &[u8] = &block;
    assert_eq!(decode_properties(&mut cursor)?, props);
    Ok(())
}

#[test]
fn the_empty_property_block_is_one_zero_byte() -> TestResult {
    assert_eq!(encoded_properties(&[])?, [0x00]);
    let mut cursor: &[u8] = &[0x00, 0xaa];
    assert_eq!(decode_properties(&mut cursor)?, []);
    // Only the block is consumed.
    assert_eq!(cursor, [0xaa]);
    Ok(())
}

#[test]
fn property_length_grows_to_a_multi_byte_variable_integer() -> TestResult {
    // 1 identifier + 2 length + 300 text = 303 = 0xaf 0x02.
    let long = encoded_properties(&[Property::ReasonString("x".repeat(300))])?;
    assert_eq!(&long[..5], [0xaf, 0x02, 0x1f, 0x01, 0x2c]);
    assert_eq!(long.len(), 2 + 303);

    // A string of the largest legal size: 1 + 2 + 65_535 = 0x82 0x80 0x04.
    let largest = encoded_properties(&[Property::ContentType("x".repeat(65_535))])?;
    assert_eq!(&largest[..6], [0x82, 0x80, 0x04, 0x03, 0xff, 0xff]);
    assert_eq!(largest.len(), 3 + 65_538);
    let mut cursor: &[u8] = &largest;
    assert_eq!(
        decode_properties(&mut cursor)?,
        [Property::ContentType("x".repeat(65_535))]
    );
    Ok(())
}

#[test]
fn subscription_identifier_uses_the_variable_byte_integer_forms() -> TestResult {
    let cases: [(u32, &[u8]); 4] = [
        (1, &[0x02, 0x0b, 0x01]),
        (127, &[0x02, 0x0b, 0x7f]),
        (128, &[0x03, 0x0b, 0x80, 0x01]),
        (16_384, &[0x04, 0x0b, 0x80, 0x80, 0x01]),
    ];
    for (id, block) in cases {
        assert_eq!(
            encoded_properties(&[Property::SubscriptionIdentifier(id)])?,
            block
        );
        let mut cursor: &[u8] = block;
        assert_eq!(
            decode_properties(&mut cursor)?,
            [Property::SubscriptionIdentifier(id)]
        );
    }
    Ok(())
}

#[test]
fn only_user_property_and_subscription_identifier_may_repeat() -> TestResult {
    for (property, bytes) in property_table() {
        let twice = [bytes.as_slice(), bytes.as_slice()].concat();
        let mut block = vec![u8::try_from(twice.len())?];
        block.extend_from_slice(&twice);
        let mut cursor: &[u8] = &block;
        let decoded = decode_properties(&mut cursor);
        if matches!(
            property,
            Property::UserProperty(..) | Property::SubscriptionIdentifier(_)
        ) {
            assert_eq!(decoded?, [property.clone(), property]);
        } else {
            assert_eq!(
                decoded,
                Err(Error::DuplicateProperty(property.identifier())),
                "{property:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn property_decoder_rejects_malformed_blocks() {
    assert_properties_rejected(&[
        // 0x07 is not assigned; identifiers are Variable Byte Integers, and
        // one that does not fit a byte is reported as 0xff.
        (&[0x02, 0x07, 0x00], Error::UnknownProperty(0x07)),
        (&[0x01, 0x00], Error::UnknownProperty(0x00)),
        (&[0x01, 0x2b], Error::UnknownProperty(0x2b)),
        (&[0x03, 0x80, 0x01, 0x00], Error::UnknownProperty(0x80)),
        (&[0x03, 0x80, 0x02, 0x00], Error::UnknownProperty(0xff)),
        (&[0x02, 0x0b, 0x00], Error::ZeroSubscriptionId),
        // The block or one of its values is cut short.
        (&[], Error::Underflow { needed: 4 }),
        (&[0x0a], Error::Underflow { needed: 10 }),
        (&[0x01, 0x01], Error::Underflow { needed: 1 }),
        (&[0x02, 0x13, 0x00], Error::Underflow { needed: 1 }),
        (&[0x02, 0x02, 0x00], Error::Underflow { needed: 3 }),
        (
            &[0x04, 0x03, 0x00, 0x05, b'a'],
            Error::Underflow { needed: 4 },
        ),
        (
            &[0x04, 0x09, 0x00, 0x02, 0x00],
            Error::Underflow { needed: 1 },
        ),
        (&[0x01, 0x80], Error::Underflow { needed: 3 }),
        (&[0x80, 0x80, 0x80, 0x80, 0x01], Error::VarIntTooLong),
        (&[0x04, 0x03, 0x00, 0x01, 0xff], Error::BadUtf8),
    ]);
}

#[test]
fn property_decoder_does_not_range_check_values() -> TestResult {
    let mut cursor: &[u8] = &[
        0x0a, 0x24, 0x07, 0x01, 0x02, 0x23, 0x00, 0x00, 0x21, 0x00, 0x00,
    ];
    assert_eq!(
        decode_properties(&mut cursor)?,
        [
            Property::MaximumQoS(7),
            Property::PayloadFormatIndicator(2),
            Property::TopicAlias(0),
            Property::ReceiveMaximum(0),
        ]
    );
    Ok(())
}

#[test]
fn property_decoder_leaves_the_cursor_behind_the_length_on_error() {
    let mut cursor: &[u8] = &[0x02, 0x07, 0x00, 0xaa];
    assert_eq!(
        decode_properties(&mut cursor),
        Err(Error::UnknownProperty(0x07))
    );
    assert_eq!(cursor, [0x07, 0x00, 0xaa]);
}

#[test]
fn property_encoder_rejects_oversized_values_without_touching_the_output() {
    let cases = [
        (
            Property::ContentType("x".repeat(65_536)),
            Error::Malformed("string exceeds 65535 bytes"),
        ),
        (
            Property::UserProperty("k".to_owned(), "x".repeat(65_536)),
            Error::Malformed("string exceeds 65535 bytes"),
        ),
        (
            Property::CorrelationData(Bytes::from(vec![0_u8; 65_536])),
            Error::Malformed("binary exceeds 65535 bytes"),
        ),
        (
            Property::SubscriptionIdentifier(268_435_456),
            Error::VarIntTooLong,
        ),
    ];
    for (property, expected) in cases {
        let mut out = BytesMut::from(&[0xaa][..]);
        assert_eq!(
            encode_properties(&[Property::TopicAlias(1), property], &mut out),
            Err(expected)
        );
        assert_eq!(&out[..], [0xaa]);
    }
}

#[test]
fn property_encoder_writes_a_zero_subscription_identifier_the_decoder_refuses() -> TestResult {
    let block = encoded_properties(&[Property::SubscriptionIdentifier(0)])?;
    assert_eq!(block, [0x02, 0x0b, 0x00]);
    assert_properties_rejected(&[(&block, Error::ZeroSubscriptionId)]);
    Ok(())
}

// ---- §3.1 CONNECT ----------------------------------------------------------------

#[test]
fn connect_frames_are_byte_exact() -> TestResult {
    assert_frame(
        &PacketV5::Connect(ConnectV5 {
            clean_start: true,
            ..connect("")
        }),
        &[
            0x10, 0x0d, // CONNECT, remaining length 13
            0x00, 0x04, b'M', b'Q', b'T', b'T', // protocol name
            0x05, // protocol version 5
            0x02, // connect flags: clean start
            0x00, 0x00, // keep alive
            0x00, // property length
            0x00, 0x00, // client id
        ],
    )?;

    assert_frame(
        &PacketV5::Connect(ConnectV5 {
            client_id: "c".to_owned(),
            clean_start: true,
            keep_alive_secs: 30,
            properties: vec![
                Property::SessionExpiryInterval(3600),
                Property::ReceiveMaximum(50),
            ],
            will: Some(Will {
                properties: vec![Property::WillDelayInterval(10)],
                ..will(QoS::AtLeastOnce, true)
            }),
            username: Some("u".to_owned()),
            password: Some(Bytes::from_static(b"pw")),
        }),
        &[
            0x10, 0x2d, // CONNECT, remaining length 45
            0x00, 0x04, b'M', b'Q', b'T', b'T', // protocol name
            0x05, // protocol version 5
            0xee, // user name, password, will retain, will QoS 1, will, clean start
            0x00, 0x1e, // keep alive 30 s
            0x08, 0x11, 0x00, 0x00, 0x0e, 0x10, 0x21, 0x00, 0x32, // properties
            0x00, 0x01, b'c', // client id
            0x05, 0x18, 0x00, 0x00, 0x00, 0x0a, // will properties
            0x00, 0x03, b'w', b'/', b't', // will topic
            0x00, 0x03, b'b', b'y', b'e', // will payload
            0x00, 0x01, b'u', // user name
            0x00, 0x02, b'p', b'w', // password
        ],
    )
}

#[test]
fn connect_flag_byte_follows_the_spec_bit_layout() -> TestResult {
    let user = || Some("u".to_owned());
    let pass = || Some(Bytes::from_static(b"p"));
    let cases = [
        (connect("c"), 0x00),
        (
            ConnectV5 {
                clean_start: true,
                ..connect("c")
            },
            0x02,
        ),
        (
            ConnectV5 {
                will: Some(will(QoS::AtMostOnce, false)),
                ..connect("c")
            },
            0x04,
        ),
        (
            ConnectV5 {
                will: Some(will(QoS::AtLeastOnce, false)),
                ..connect("c")
            },
            0x0c,
        ),
        (
            ConnectV5 {
                will: Some(will(QoS::ExactlyOnce, false)),
                ..connect("c")
            },
            0x14,
        ),
        (
            ConnectV5 {
                will: Some(will(QoS::ExactlyOnce, true)),
                ..connect("c")
            },
            0x34,
        ),
        (
            ConnectV5 {
                username: user(),
                ..connect("c")
            },
            0x80,
        ),
        (
            ConnectV5 {
                password: pass(),
                ..connect("c")
            },
            0x40,
        ),
        (
            ConnectV5 {
                clean_start: true,
                will: Some(will(QoS::ExactlyOnce, true)),
                username: user(),
                password: pass(),
                ..connect("c")
            },
            0xf6,
        ),
    ];
    for (packet, flags) in cases {
        let packet = PacketV5::Connect(packet);
        let encoded = encode_v5(&packet)?;
        // Fixed header (2) + protocol name (6) + version (1).
        assert_eq!(encoded.get(9), Some(&flags), "{packet:?}");
        assert_eq!(decode_v5(&encoded)?, (packet, encoded.len()));
    }
    Ok(())
}

#[test]
fn connect_decoder_checks_the_flags_against_the_payload() -> TestResult {
    const RESERVED_BIT: Error = Error::Malformed("CONNECT reserved flag bit 0 must be 0");
    const WILL_BITS: Error = Error::Malformed("Will QoS/Retain set without Will Flag");
    // Keep alive, empty property block, empty client id.
    let plain: &[u8] = &[0x00, 0x00, 0x00, 0x00, 0x00];
    // The same, then an empty will property block, will topic "t" and an
    // empty will payload.
    let with_will: &[u8] = &[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, b't', 0x00, 0x00,
    ];
    // `(connect flags, bytes after the flags, error)`.
    let cases: [(u8, &[u8], Error); 10] = [
        (0x03, plain, RESERVED_BIT),
        // The reserved bit is checked before the will bits.
        (0x0b, plain, RESERVED_BIT),
        (0x0a, plain, WILL_BITS),
        (0x22, plain, WILL_BITS),
        // Will QoS 3 is only noticed after the will fields have been read.
        (0x1e, with_will, Error::BadQoS(3)),
        (0x1e, plain, Error::Underflow { needed: 4 }),
        // A flagged user name / password that is not in the payload.
        (0x82, plain, Error::Underflow { needed: 2 }),
        (0x42, plain, Error::Underflow { needed: 2 }),
        // Nothing, or only the keep alive, after the flags.
        (0x02, &[], Error::Underflow { needed: 2 }),
        (0x02, &[0x00, 0x00], Error::Underflow { needed: 4 }),
    ];
    for (flags, tail, expected) in cases {
        let mut body = vec![0x00, 0x04, b'M', b'Q', b'T', b'T', 0x05, flags];
        body.extend_from_slice(tail);
        let mut frame = vec![0x10, u8::try_from(body.len())?];
        frame.extend_from_slice(&body);
        assert_eq!(decode_v5(&frame).err(), Some(expected), "{frame:02x?}");
    }
    Ok(())
}

#[test]
fn connect_decoder_rejects_other_protocols() {
    assert_rejected(&[
        (
            &[
                0x10, 0x0f, 0x00, 0x06, b'M', b'Q', b'I', b's', b'd', b'p', 0x05, 0x02, 0x00, 0x00,
                0x00, 0x00, 0x00,
            ],
            Error::BadProtocolName("MQIsdp".to_owned()),
        ),
        (
            &[
                0x10, 0x0d, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x04, 0x02, 0x00, 0x00, 0x00, 0x00,
                0x00,
            ],
            Error::BadProtocolLevel(4),
        ),
        (
            &[0x10, 0x06, 0x00, 0x04, b'M', b'Q', b'T', b'T'],
            Error::Underflow { needed: 1 },
        ),
    ]);
}

#[test]
fn connect_decoder_ignores_bytes_after_the_last_payload_field() -> TestResult {
    let frame = [
        0x10, 0x0f, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x05, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
        0xde, 0xad,
    ];
    assert_eq!(
        decode_v5(&frame)?,
        (
            PacketV5::Connect(ConnectV5 {
                clean_start: true,
                ..connect("")
            }),
            frame.len()
        )
    );
    Ok(())
}

// ---- §3.2 CONNACK ----------------------------------------------------------------

#[test]
fn connack_frames_are_byte_exact() -> TestResult {
    assert_frame(
        &PacketV5::ConnAck(ConnAckV5 {
            session_present: false,
            reason_code: ReasonCode::Success,
            properties: vec![],
        }),
        &[0x20, 0x03, 0x00, 0x00, 0x00],
    )?;
    assert_frame(
        &PacketV5::ConnAck(ConnAckV5 {
            session_present: false,
            reason_code: ReasonCode::BadUserNameOrPassword,
            properties: vec![],
        }),
        &[0x20, 0x03, 0x00, 0x86, 0x00],
    )?;
    assert_frame(
        &PacketV5::ConnAck(ConnAckV5 {
            session_present: true,
            reason_code: ReasonCode::Success,
            properties: vec![
                Property::AssignedClientIdentifier("auto-1".to_owned()),
                Property::MaximumQoS(1),
            ],
        }),
        &[
            0x20, 0x0e, 0x01, 0x00, 0x0b, 0x12, 0x00, 0x06, b'a', b'u', b't', b'o', b'-', b'1',
            0x24, 0x01,
        ],
    )
}

#[test]
fn connack_decoder_reads_bit_zero_of_the_flags_and_needs_a_property_length() -> TestResult {
    assert_eq!(
        decode_v5(&[0x20, 0x03, 0xff, 0x00, 0x00])?,
        (
            PacketV5::ConnAck(ConnAckV5 {
                session_present: true,
                reason_code: ReasonCode::Success,
                properties: vec![],
            }),
            5
        )
    );
    assert_rejected(&[
        (&[0x20, 0x02, 0x00, 0x00], Error::Underflow { needed: 4 }),
        (&[0x20, 0x01, 0x00], Error::Underflow { needed: 1 }),
        (&[0x20, 0x00], Error::Underflow { needed: 1 }),
        (
            &[0x20, 0x03, 0x00, 0x03, 0x00],
            Error::BadReasonCode {
                packet: "CONNACK",
                code: 0x03,
            },
        ),
    ]);
    Ok(())
}

// ---- §3.3 PUBLISH ----------------------------------------------------------------

#[test]
fn publish_frames_are_byte_exact() -> TestResult {
    assert_frame(
        &PacketV5::Publish(PublishV5 {
            properties: vec![Property::PayloadFormatIndicator(1), Property::TopicAlias(4)],
            payload: Bytes::from_static(b"hi"),
            ..publish(QoS::AtLeastOnce, (false, false), Some(7))
        }),
        &[
            0x32, 0x0d, // PUBLISH QoS 1, remaining length 13
            0x00, 0x01, b't', // topic
            0x00, 0x07, // packet identifier
            0x05, 0x01, 0x01, 0x23, 0x00, 0x04, // properties
            b'h', b'i', // payload
        ],
    )?;
    assert_frame(
        &PacketV5::Publish(PublishV5 {
            payload: Bytes::from_static(b"ON"),
            ..publish(QoS::AtMostOnce, (false, true), None)
        }),
        &[0x31, 0x06, 0x00, 0x01, b't', 0x00, b'O', b'N'],
    )?;
    assert_frame(
        &PacketV5::Publish(publish(QoS::AtMostOnce, (false, false), None)),
        &[0x30, 0x04, 0x00, 0x01, b't', 0x00],
    )?;
    assert_frame(
        &PacketV5::Publish(publish(QoS::AtLeastOnce, (true, false), Some(1))),
        &[0x3a, 0x06, 0x00, 0x01, b't', 0x00, 0x01, 0x00],
    )?;
    assert_frame(
        &PacketV5::Publish(publish(QoS::ExactlyOnce, (false, false), Some(2))),
        &[0x34, 0x06, 0x00, 0x01, b't', 0x00, 0x02, 0x00],
    )?;
    assert_frame(
        &PacketV5::Publish(publish(QoS::ExactlyOnce, (true, true), Some(9))),
        &[0x3d, 0x06, 0x00, 0x01, b't', 0x00, 0x09, 0x00],
    )
}

#[test]
fn publish_encoder_needs_a_packet_id_above_qos0_and_drops_it_at_qos0() -> TestResult {
    for qos in [QoS::AtLeastOnce, QoS::ExactlyOnce] {
        assert_eq!(
            encode_v5(&PacketV5::Publish(publish(qos, (false, false), None))),
            Err(Error::Malformed("PUBLISH QoS > 0 missing packet id"))
        );
    }
    let stray = encode_v5(&PacketV5::Publish(publish(
        QoS::AtMostOnce,
        (false, false),
        Some(7),
    )))?;
    assert_eq!(&stray[..], [0x30, 0x04, 0x00, 0x01, b't', 0x00]);
    Ok(())
}

#[test]
fn publish_encoder_writes_dup_on_qos0_that_the_decoder_refuses() -> TestResult {
    let frame = encode_v5(&PacketV5::Publish(publish(
        QoS::AtMostOnce,
        (true, false),
        None,
    )))?;
    assert_eq!(&frame[..], [0x38, 0x04, 0x00, 0x01, b't', 0x00]);
    assert_rejected(&[(
        &frame,
        Error::Malformed("PUBLISH DUP must be 0 for QoS 0 (§3.3.1.1)"),
    )]);
    Ok(())
}

#[test]
fn publish_decoder_rejects_malformed_frames() {
    assert_rejected(&[
        (&[0x36, 0x04, 0x00, 0x01, b't', 0x00], Error::BadQoS(3)),
        // The QoS bits are checked before DUP.
        (&[0x3e, 0x04, 0x00, 0x01, b't', 0x00], Error::BadQoS(3)),
        (
            &[0x39, 0x04, 0x00, 0x01, b't', 0x00],
            Error::Malformed("PUBLISH DUP must be 0 for QoS 0 (§3.3.1.1)"),
        ),
        // No packet identifier, then no property length.
        (
            &[0x32, 0x03, 0x00, 0x01, b't'],
            Error::Underflow { needed: 2 },
        ),
        (
            &[0x30, 0x03, 0x00, 0x01, b't'],
            Error::Underflow { needed: 4 },
        ),
        (&[0x30, 0x00], Error::Underflow { needed: 2 }),
        (&[0x30, 0x04, 0x00, 0x01, 0xff, 0x00], Error::BadUtf8),
        (
            &[0x30, 0x06, 0x00, 0x01, b't', 0x02, 0x07, 0x00],
            Error::UnknownProperty(0x07),
        ),
    ]);
}

// ---- §3.4-§3.7 PUBACK / PUBREC / PUBREL / PUBCOMP ---------------------------------

#[test]
fn successful_acknowledgements_without_properties_use_the_two_byte_form() -> TestResult {
    let success = ReasonCode::Success;
    assert_frame(
        &PacketV5::PubAck(PubAckV5 {
            packet_id: 42,
            reason_code: success,
            properties: vec![],
        }),
        &[0x40, 0x02, 0x00, 0x2a],
    )?;
    assert_frame(
        &PacketV5::PubRec(PubRecV5 {
            packet_id: 9,
            reason_code: success,
            properties: vec![],
        }),
        &[0x50, 0x02, 0x00, 0x09],
    )?;
    // §3.6.1: PUBREL carries the fixed-header flags 0b0010.
    assert_frame(
        &PacketV5::PubRel(PubRelV5 {
            packet_id: 9,
            reason_code: success,
            properties: vec![],
        }),
        &[0x62, 0x02, 0x00, 0x09],
    )?;
    assert_frame(
        &PacketV5::PubComp(PubCompV5 {
            packet_id: 0xffff,
            reason_code: success,
            properties: vec![],
        }),
        &[0x70, 0x02, 0xff, 0xff],
    )
}

#[test]
fn other_acknowledgements_carry_reason_code_and_property_length() -> TestResult {
    assert_frame(
        &PacketV5::PubAck(PubAckV5 {
            packet_id: 42,
            reason_code: ReasonCode::NoMatchingSubscribers,
            properties: vec![Property::ReasonString("no".to_owned())],
        }),
        &[
            0x40, 0x09, 0x00, 0x2a, 0x10, 0x05, 0x1f, 0x00, 0x02, b'n', b'o',
        ],
    )?;
    // Success with a property cannot use the short form.
    assert_frame(
        &PacketV5::PubRec(PubRecV5 {
            packet_id: 9,
            reason_code: ReasonCode::Success,
            properties: vec![Property::ReasonString("x".to_owned())],
        }),
        &[0x50, 0x08, 0x00, 0x09, 0x00, 0x04, 0x1f, 0x00, 0x01, b'x'],
    )?;
    assert_frame(
        &PacketV5::PubRel(PubRelV5 {
            packet_id: 9,
            reason_code: ReasonCode::PacketIdentifierNotFound,
            properties: vec![],
        }),
        &[0x62, 0x04, 0x00, 0x09, 0x92, 0x00],
    )?;
    assert_frame(
        &PacketV5::PubComp(PubCompV5 {
            packet_id: 9,
            reason_code: ReasonCode::PacketIdentifierNotFound,
            properties: vec![],
        }),
        &[0x70, 0x04, 0x00, 0x09, 0x92, 0x00],
    )
}

#[test]
fn acknowledgement_decoder_accepts_the_shortened_forms() -> TestResult {
    // §3.4.2.1: the property length may be left out after a reason code.
    assert_eq!(
        decode_v5(&[0x40, 0x03, 0x00, 0x2a, 0x10])?,
        (
            PacketV5::PubAck(PubAckV5 {
                packet_id: 42,
                reason_code: ReasonCode::NoMatchingSubscribers,
                properties: vec![],
            }),
            5
        )
    );
    // An explicit Success with an empty property block.
    assert_eq!(
        decode_v5(&[0x70, 0x04, 0x00, 0x2a, 0x00, 0x00])?,
        (
            PacketV5::PubComp(PubCompV5 {
                packet_id: 42,
                reason_code: ReasonCode::Success,
                properties: vec![],
            }),
            6
        )
    );
    // Reserved flags are only checked for PUBREL.
    assert_eq!(
        decode_v5(&[0x4f, 0x02, 0x00, 0x09])?,
        (
            PacketV5::PubAck(PubAckV5 {
                packet_id: 9,
                reason_code: ReasonCode::Success,
                properties: vec![],
            }),
            4
        )
    );
    Ok(())
}

#[test]
fn acknowledgement_decoder_rejects_malformed_frames() {
    let bad = |packet: &'static str| Error::BadReasonCode { packet, code: 0x03 };
    assert_rejected(&[
        (&[0x40, 0x03, 0x00, 0x09, 0x03], bad("PUBACK")),
        (&[0x50, 0x03, 0x00, 0x09, 0x03], bad("PUBREC")),
        (&[0x62, 0x03, 0x00, 0x09, 0x03], bad("PUBREL")),
        (&[0x70, 0x03, 0x00, 0x09, 0x03], bad("PUBCOMP")),
        (&[0x60, 0x02, 0x00, 0x09], RESERVED_FLAGS),
        (&[0x6f, 0x02, 0x00, 0x09], RESERVED_FLAGS),
        // The flag check comes before the body is read.
        (&[0x60, 0x00], RESERVED_FLAGS),
        (&[0x40, 0x00], Error::Underflow { needed: 2 }),
        (&[0x62, 0x01, 0x00], Error::Underflow { needed: 1 }),
        (
            &[0x40, 0x05, 0x00, 0x09, 0x00, 0x02, 0x1f],
            Error::Underflow { needed: 1 },
        ),
    ]);
}

// ---- §3.8-§3.11 SUBSCRIBE / SUBACK / UNSUBSCRIBE / UNSUBACK -----------------------

#[test]
fn subscribe_frame_is_byte_exact() -> TestResult {
    assert_frame(
        &PacketV5::Subscribe(SubscribeV5 {
            packet_id: 11,
            properties: vec![Property::SubscriptionIdentifier(5)],
            subscriptions: vec![
                subscription(
                    "a/+",
                    QoS::ExactlyOnce,
                    (true, true, RetainHandling::SendIfNew),
                ),
                subscription(
                    "#",
                    QoS::AtMostOnce,
                    (false, false, RetainHandling::DoNotSend),
                ),
            ],
        }),
        &[
            0x82, 0x0f, // SUBSCRIBE, remaining length 15
            0x00, 0x0b, // packet identifier
            0x02, 0x0b, 0x05, // properties
            0x00, 0x03, b'a', b'/', b'+', 0x1e, // QoS 2, NL, RAP, retain handling 1
            0x00, 0x01, b'#', 0x20, // QoS 0, retain handling 2
        ],
    )
}

#[test]
fn subscription_options_byte_follows_the_spec_bit_layout() -> TestResult {
    let send = RetainHandling::SendOnSubscribe;
    let cases = [
        (QoS::AtMostOnce, (false, false, send), 0x00),
        (QoS::AtLeastOnce, (false, false, send), 0x01),
        (QoS::ExactlyOnce, (false, false, send), 0x02),
        (QoS::AtMostOnce, (true, false, send), 0x04),
        (QoS::AtMostOnce, (false, true, send), 0x08),
        (
            QoS::AtMostOnce,
            (false, false, RetainHandling::SendIfNew),
            0x10,
        ),
        (
            QoS::AtMostOnce,
            (false, false, RetainHandling::DoNotSend),
            0x20,
        ),
        (
            QoS::ExactlyOnce,
            (true, true, RetainHandling::DoNotSend),
            0x2e,
        ),
    ];
    for (qos, options, byte) in cases {
        assert_frame(
            &PacketV5::Subscribe(SubscribeV5 {
                packet_id: 1,
                properties: vec![],
                subscriptions: vec![subscription("a", qos, options)],
            }),
            &[0x82, 0x07, 0x00, 0x01, 0x00, 0x00, 0x01, b'a', byte],
        )?;
    }
    Ok(())
}

#[test]
fn subscribe_decoder_rejects_malformed_frames() {
    const RESERVED_BITS: Error =
        Error::Malformed("subscription options reserved bits 6-7 must be 0");
    const RETAIN_HANDLING: Error = Error::Malformed("invalid Retain Handling value 3 (§3.8.3.1)");
    let with_options = |byte: u8| [0x82, 0x07, 0x00, 0x01, 0x00, 0x00, 0x01, b'a', byte];
    let cases = [
        (0x40, RESERVED_BITS),
        (0x80, RESERVED_BITS),
        // Reserved bits are checked first, then QoS, then retain handling.
        (0xff, RESERVED_BITS),
        (0x03, Error::BadQoS(3)),
        (0x33, Error::BadQoS(3)),
        (0x30, RETAIN_HANDLING),
        (0x3e, RETAIN_HANDLING),
    ];
    for (byte, expected) in cases {
        assert_eq!(
            decode_v5(&with_options(byte)).err(),
            Some(expected),
            "options {byte:#04x}"
        );
    }
    assert_rejected(&[
        (
            &[0x80, 0x07, 0x00, 0x01, 0x00, 0x00, 0x01, b'a', 0x00],
            RESERVED_FLAGS,
        ),
        (
            &[0x82, 0x03, 0x00, 0x01, 0x00],
            Error::Malformed("SUBSCRIBE must contain at least one topic filter"),
        ),
        (
            &[0x82, 0x06, 0x00, 0x01, 0x00, 0x00, 0x01, b'a'],
            Error::Underflow { needed: 1 },
        ),
        (&[0x82, 0x02, 0x00, 0x01], Error::Underflow { needed: 4 }),
    ]);
}

#[test]
fn suback_and_unsuback_frames_are_byte_exact() -> TestResult {
    assert_frame(
        &PacketV5::SubAck(SubAckV5 {
            packet_id: 11,
            properties: vec![],
            reason_codes: vec![
                ReasonCode::GrantedQoS2,
                ReasonCode::GrantedQoS1,
                ReasonCode::Success,
                ReasonCode::TopicFilterInvalid,
                ReasonCode::NotAuthorized,
            ],
        }),
        &[0x90, 0x08, 0x00, 0x0b, 0x00, 0x02, 0x01, 0x00, 0x8f, 0x87],
    )?;
    assert_frame(
        &PacketV5::SubAck(SubAckV5 {
            packet_id: 11,
            properties: vec![Property::ReasonString("x".to_owned())],
            reason_codes: vec![],
        }),
        &[0x90, 0x07, 0x00, 0x0b, 0x04, 0x1f, 0x00, 0x01, b'x'],
    )?;
    assert_frame(
        &PacketV5::UnsubAck(UnsubAckV5 {
            packet_id: 21,
            properties: vec![],
            reason_codes: vec![ReasonCode::Success, ReasonCode::NoSubscriptionExisted],
        }),
        &[0xb0, 0x05, 0x00, 0x15, 0x00, 0x00, 0x11],
    )
}

#[test]
fn unsubscribe_frame_is_byte_exact() -> TestResult {
    assert_frame(
        &PacketV5::Unsubscribe(UnsubscribeV5 {
            packet_id: 21,
            properties: vec![Property::UserProperty("x".to_owned(), "y".to_owned())],
            topic_filters: vec!["a/+".to_owned(), "#".to_owned()],
        }),
        &[
            0xa2, 0x12, // UNSUBSCRIBE, remaining length 18
            0x00, 0x15, // packet identifier
            0x07, 0x26, 0x00, 0x01, b'x', 0x00, 0x01, b'y', // properties
            0x00, 0x03, b'a', b'/', b'+', // first filter
            0x00, 0x01, b'#', // second filter
        ],
    )
}

#[test]
fn suback_unsubscribe_and_unsuback_decoders_reject_malformed_frames() {
    assert_rejected(&[
        (
            &[0x90, 0x04, 0x00, 0x0b, 0x00, 0x03],
            Error::BadReasonCode {
                packet: "SUBACK",
                code: 0x03,
            },
        ),
        (
            &[0xb0, 0x05, 0x00, 0x15, 0x00, 0x00, 0x7f],
            Error::BadReasonCode {
                packet: "UNSUBACK",
                code: 0x7f,
            },
        ),
        (&[0x90, 0x02, 0x00, 0x0b], Error::Underflow { needed: 4 }),
        (&[0xb0, 0x01, 0x00], Error::Underflow { needed: 1 }),
        (
            &[0xa0, 0x06, 0x00, 0x15, 0x00, 0x00, 0x01, b'a'],
            RESERVED_FLAGS,
        ),
        (
            &[0xa2, 0x03, 0x00, 0x15, 0x00],
            Error::Malformed("UNSUBSCRIBE must contain at least one topic filter"),
        ),
        (
            &[0xa2, 0x05, 0x00, 0x15, 0x00, 0x00, 0x02],
            Error::Underflow { needed: 2 },
        ),
    ]);
}

// ---- §3.12-§3.15 PINGREQ / PINGRESP / DISCONNECT / AUTH ---------------------------

#[test]
fn ping_frames_are_two_bytes() -> TestResult {
    assert_frame(&PacketV5::PingReq, &[0xc0, 0x00])?;
    assert_frame(&PacketV5::PingResp, &[0xd0, 0x00])
}

#[test]
fn ping_decoder_ignores_flags_and_body() -> TestResult {
    assert_eq!(decode_v5(&[0xc5, 0x01, 0xff])?, (PacketV5::PingReq, 3));
    assert_eq!(
        decode_v5(&[0xd0, 0x02, 0x00, 0x00])?,
        (PacketV5::PingResp, 4)
    );
    Ok(())
}

#[test]
fn disconnect_frames_are_byte_exact() -> TestResult {
    // §3.14.2.1: remaining length 0 means Normal disconnection.
    assert_frame(
        &PacketV5::Disconnect(DisconnectV5 {
            reason_code: ReasonCode::Success,
            properties: vec![],
        }),
        &[0xe0, 0x00],
    )?;
    assert_frame(
        &PacketV5::Disconnect(DisconnectV5 {
            reason_code: ReasonCode::DisconnectWithWill,
            properties: vec![],
        }),
        &[0xe0, 0x02, 0x04, 0x00],
    )?;
    assert_frame(
        &PacketV5::Disconnect(DisconnectV5 {
            reason_code: ReasonCode::SessionTakenOver,
            properties: vec![Property::ReasonString("x".to_owned())],
        }),
        &[0xe0, 0x06, 0x8e, 0x04, 0x1f, 0x00, 0x01, b'x'],
    )?;
    assert_frame(
        &PacketV5::Disconnect(DisconnectV5 {
            reason_code: ReasonCode::Success,
            properties: vec![Property::SessionExpiryInterval(0)],
        }),
        &[0xe0, 0x07, 0x00, 0x05, 0x11, 0x00, 0x00, 0x00, 0x00],
    )
}

#[test]
fn auth_frames_are_byte_exact() -> TestResult {
    assert_frame(
        &PacketV5::Auth(AuthV5 {
            reason_code: ReasonCode::Success,
            properties: vec![],
        }),
        &[0xf0, 0x00],
    )?;
    assert_frame(
        &PacketV5::Auth(AuthV5 {
            reason_code: ReasonCode::ReAuthenticate,
            properties: vec![],
        }),
        &[0xf0, 0x02, 0x19, 0x00],
    )?;
    assert_frame(
        &PacketV5::Auth(AuthV5 {
            reason_code: ReasonCode::ContinueAuthentication,
            properties: vec![
                Property::AuthenticationMethod("m".to_owned()),
                Property::AuthenticationData(Bytes::from_static(&[0x01])),
            ],
        }),
        &[
            0xf0, 0x0a, 0x18, 0x08, 0x15, 0x00, 0x01, b'm', 0x16, 0x00, 0x01, 0x01,
        ],
    )
}

#[test]
fn disconnect_and_auth_decoders_accept_a_lone_reason_code_and_reject_unknown_ones() -> TestResult {
    assert_eq!(
        decode_v5(&[0xe0, 0x01, 0x8e])?,
        (
            PacketV5::Disconnect(DisconnectV5 {
                reason_code: ReasonCode::SessionTakenOver,
                properties: vec![],
            }),
            3
        )
    );
    assert_eq!(
        decode_v5(&[0xf0, 0x01, 0x18])?,
        (
            PacketV5::Auth(AuthV5 {
                reason_code: ReasonCode::ContinueAuthentication,
                properties: vec![],
            }),
            3
        )
    );
    assert_rejected(&[
        (
            &[0xe0, 0x01, 0x03],
            Error::BadReasonCode {
                packet: "DISCONNECT",
                code: 0x03,
            },
        ),
        (
            &[0xf0, 0x01, 0x03],
            Error::BadReasonCode {
                packet: "AUTH",
                code: 0x03,
            },
        ),
        (&[0xe0, 0x02, 0x00, 0x05], Error::Underflow { needed: 5 }),
    ]);
    Ok(())
}

// ---- §2.1 fixed header --------------------------------------------------------------

#[test]
fn fixed_header_errors_are_reported_before_the_packet_type_is_looked_at() {
    const UNKNOWN: Error = Error::Malformed("unknown MQTT control packet type");
    assert_rejected(&[
        (&[], Error::Underflow { needed: 2 }),
        (&[0xc0], Error::Underflow { needed: 4 }),
        (&[0x30, 0x80], Error::Underflow { needed: 3 }),
        (&[0x30, 0x80, 0x80, 0x80, 0x80], Error::VarIntTooLong),
        (&[0x40, 0x02, 0x00], Error::Underflow { needed: 1 }),
        (
            &[0x30, 0xff, 0xff, 0xff, 0x7f],
            Error::Underflow {
                needed: 268_435_455,
            },
        ),
        // Type 0 is reserved (§2.1.2); it is only noticed on a whole frame.
        (&[0x00], Error::Underflow { needed: 4 }),
        (&[0x00, 0x05], Error::Underflow { needed: 5 }),
        (&[0x00, 0x00], UNKNOWN),
        (&[0x0f, 0x01, 0x00], UNKNOWN),
    ]);
}

#[test]
fn decoder_consumes_exactly_one_frame() -> TestResult {
    let two = [0xc0, 0x00, 0xe0, 0x00, 0xff];
    assert_eq!(decode_v5(&two)?, (PacketV5::PingReq, 2));
    assert_eq!(
        decode_v5(&two[2..])?,
        (
            PacketV5::Disconnect(DisconnectV5 {
                reason_code: ReasonCode::Success,
                properties: vec![],
            }),
            2
        )
    );
    Ok(())
}

#[test]
fn encoder_rejects_strings_and_binary_data_beyond_65535_bytes() {
    let too_long = "x".repeat(65_536);
    let string = Err(Error::Malformed("string exceeds 65535 bytes"));
    let binary = Err(Error::Malformed("binary exceeds 65535 bytes"));
    assert_eq!(
        encode_v5(&PacketV5::Publish(PublishV5 {
            topic: too_long.clone(),
            ..publish(QoS::AtMostOnce, (false, false), None)
        })),
        string
    );
    assert_eq!(encode_v5(&PacketV5::Connect(connect(&too_long))), string);
    assert_eq!(
        encode_v5(&PacketV5::Unsubscribe(UnsubscribeV5 {
            packet_id: 1,
            properties: vec![],
            topic_filters: vec![too_long.clone()],
        })),
        string
    );
    assert_eq!(
        encode_v5(&PacketV5::Connect(ConnectV5 {
            password: Some(Bytes::from(too_long.clone().into_bytes())),
            ..connect("c")
        })),
        binary
    );
    assert_eq!(
        encode_v5(&PacketV5::Connect(ConnectV5 {
            will: Some(Will {
                payload: Bytes::from(too_long.into_bytes()),
                ..will(QoS::AtMostOnce, false)
            }),
            ..connect("c")
        })),
        binary
    );
}

#[test]
fn a_65535_byte_topic_round_trips() -> TestResult {
    let topic = "t".repeat(65_535);
    // Remaining length 2 + 65_535 + 1 = 65_538 = 0x82 0x80 0x04.
    let mut frame = vec![0x30, 0x82, 0x80, 0x04, 0xff, 0xff];
    frame.extend_from_slice(topic.as_bytes());
    frame.push(0x00);
    assert_frame(
        &PacketV5::Publish(PublishV5 {
            topic,
            ..publish(QoS::AtMostOnce, (false, false), None)
        }),
        &frame,
    )
}

// ---- value tables -------------------------------------------------------------------

/// The complete §2.4 Reason Code table.
const REASON_CODES: [(u8, ReasonCode); 43] = [
    (0x00, ReasonCode::Success),
    (0x01, ReasonCode::GrantedQoS1),
    (0x02, ReasonCode::GrantedQoS2),
    (0x04, ReasonCode::DisconnectWithWill),
    (0x10, ReasonCode::NoMatchingSubscribers),
    (0x11, ReasonCode::NoSubscriptionExisted),
    (0x18, ReasonCode::ContinueAuthentication),
    (0x19, ReasonCode::ReAuthenticate),
    (0x80, ReasonCode::UnspecifiedError),
    (0x81, ReasonCode::MalformedPacket),
    (0x82, ReasonCode::ProtocolError),
    (0x83, ReasonCode::ImplementationSpecificError),
    (0x84, ReasonCode::UnsupportedProtocolVersion),
    (0x85, ReasonCode::ClientIdentifierNotValid),
    (0x86, ReasonCode::BadUserNameOrPassword),
    (0x87, ReasonCode::NotAuthorized),
    (0x88, ReasonCode::ServerUnavailable),
    (0x89, ReasonCode::ServerBusy),
    (0x8a, ReasonCode::Banned),
    (0x8b, ReasonCode::ServerShuttingDown),
    (0x8c, ReasonCode::BadAuthenticationMethod),
    (0x8d, ReasonCode::KeepAliveTimeout),
    (0x8e, ReasonCode::SessionTakenOver),
    (0x8f, ReasonCode::TopicFilterInvalid),
    (0x90, ReasonCode::TopicNameInvalid),
    (0x91, ReasonCode::PacketIdentifierInUse),
    (0x92, ReasonCode::PacketIdentifierNotFound),
    (0x93, ReasonCode::ReceiveMaximumExceeded),
    (0x94, ReasonCode::TopicAliasInvalid),
    (0x95, ReasonCode::PacketTooLarge),
    (0x96, ReasonCode::MessageRateTooHigh),
    (0x97, ReasonCode::QuotaExceeded),
    (0x98, ReasonCode::AdministrativeAction),
    (0x99, ReasonCode::PayloadFormatInvalid),
    (0x9a, ReasonCode::RetainNotSupported),
    (0x9b, ReasonCode::QoSNotSupported),
    (0x9c, ReasonCode::UseAnotherServer),
    (0x9d, ReasonCode::ServerMoved),
    (0x9e, ReasonCode::SharedSubscriptionsNotSupported),
    (0x9f, ReasonCode::ConnectionRateExceeded),
    (0xa0, ReasonCode::MaximumConnectTime),
    (0xa1, ReasonCode::SubscriptionIdentifiersNotSupported),
    (0xa2, ReasonCode::WildcardSubscriptionsNotSupported),
];

#[test]
fn reason_code_bytes_match_the_spec_table() {
    for byte in 0..=u8::MAX {
        let expected = REASON_CODES
            .iter()
            .find(|(b, _)| *b == byte)
            .map(|(_, code)| *code);
        assert_eq!(ReasonCode::from_u8(byte), expected, "byte {byte:#04x}");
    }
    for (byte, code) in REASON_CODES {
        assert_eq!(code as u8, byte, "{code:?}");
        // §2.4: 0x80 and above report a failure.
        assert_eq!(code.is_error(), byte >= 0x80, "{code:?}");
    }
}

#[test]
fn retain_handling_and_packet_type_values_match_the_spec_tables() {
    for byte in 0..=u8::MAX {
        let expected = match byte {
            0 => Some(RetainHandling::SendOnSubscribe),
            1 => Some(RetainHandling::SendIfNew),
            2 => Some(RetainHandling::DoNotSend),
            _ => None,
        };
        assert_eq!(RetainHandling::from_u8(byte), expected, "byte {byte}");
    }
    let types = [
        (PacketTypeV5::Connect, 1),
        (PacketTypeV5::ConnAck, 2),
        (PacketTypeV5::Publish, 3),
        (PacketTypeV5::PubAck, 4),
        (PacketTypeV5::PubRec, 5),
        (PacketTypeV5::PubRel, 6),
        (PacketTypeV5::PubComp, 7),
        (PacketTypeV5::Subscribe, 8),
        (PacketTypeV5::SubAck, 9),
        (PacketTypeV5::Unsubscribe, 10),
        (PacketTypeV5::UnsubAck, 11),
        (PacketTypeV5::PingReq, 12),
        (PacketTypeV5::PingResp, 13),
        (PacketTypeV5::Disconnect, 14),
        (PacketTypeV5::Auth, 15),
    ];
    for (packet_type, nibble) in types {
        assert_eq!(packet_type as u8, nibble, "{packet_type:?}");
    }
}

#[test]
fn packets_report_the_type_of_their_first_header_nibble() -> TestResult {
    let success = ReasonCode::Success;
    let packets = [
        PacketV5::Connect(connect("c")),
        PacketV5::ConnAck(ConnAckV5 {
            session_present: false,
            reason_code: success,
            properties: vec![],
        }),
        PacketV5::Publish(publish(QoS::AtMostOnce, (false, false), None)),
        PacketV5::PubAck(PubAckV5 {
            packet_id: 1,
            reason_code: success,
            properties: vec![],
        }),
        PacketV5::PubRec(PubRecV5 {
            packet_id: 1,
            reason_code: success,
            properties: vec![],
        }),
        PacketV5::PubRel(PubRelV5 {
            packet_id: 1,
            reason_code: success,
            properties: vec![],
        }),
        PacketV5::PubComp(PubCompV5 {
            packet_id: 1,
            reason_code: success,
            properties: vec![],
        }),
        PacketV5::Subscribe(SubscribeV5 {
            packet_id: 1,
            properties: vec![],
            subscriptions: vec![],
        }),
        PacketV5::SubAck(SubAckV5 {
            packet_id: 1,
            properties: vec![],
            reason_codes: vec![],
        }),
        PacketV5::Unsubscribe(UnsubscribeV5 {
            packet_id: 1,
            properties: vec![],
            topic_filters: vec![],
        }),
        PacketV5::UnsubAck(UnsubAckV5 {
            packet_id: 1,
            properties: vec![],
            reason_codes: vec![],
        }),
        PacketV5::PingReq,
        PacketV5::PingResp,
        PacketV5::Disconnect(DisconnectV5 {
            reason_code: success,
            properties: vec![],
        }),
        PacketV5::Auth(AuthV5 {
            reason_code: success,
            properties: vec![],
        }),
    ];
    for (packet, nibble) in packets.iter().zip(1_u8..) {
        assert_eq!(packet.packet_type() as u8, nibble, "{packet:?}");
        let encoded = encode_v5(packet)?;
        assert_eq!(encoded.first().map(|b| b >> 4), Some(nibble), "{packet:?}");
    }
    Ok(())
}

// ---- errors -------------------------------------------------------------------------

#[test]
fn v5_error_messages_are_stable() {
    let cases = [
        (
            Error::Underflow { needed: 1 },
            "buffer underflow: need 1 more byte(s)",
        ),
        (
            Error::VarIntTooLong,
            "variable-length integer exceeds the 4-byte / 268435455 maximum",
        ),
        (
            Error::BadUtf8,
            "UTF-8 string field is not valid UTF-8 (§1.5.4)",
        ),
        (
            Error::UnknownProperty(0x07),
            "unknown property identifier 0x07 (§2.2.2.2)",
        ),
        (
            Error::DuplicateProperty(0x01),
            "property identifier 0x01 appears more than once but is not repeatable",
        ),
        (
            Error::PropertyLengthMismatch {
                declared: 3,
                consumed: 2,
            },
            "property block length 3 disagrees with the bytes consumed 2",
        ),
        (
            Error::ZeroSubscriptionId,
            "Subscription Identifier value 0 is prohibited (§3.3.2.3.8)",
        ),
        (Error::Malformed("why"), "malformed packet: why"),
        (
            Error::BadReasonCode {
                packet: "PUBACK",
                code: 0x03,
            },
            "PUBACK carries reserved/invalid reason code 0x03",
        ),
        (
            Error::BadProtocolLevel(4),
            "unsupported protocol level 4 (MQTT 5.0 codec requires level 5)",
        ),
        (Error::BadQoS(3), "unknown QoS value 3"),
        (
            Error::BadProtocolName("MQIsdp".to_owned()),
            "malformed protocol name \"MQIsdp\" (expected \"MQTT\")",
        ),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
}

#[test]
fn codec_errors_convert_into_v5_errors() {
    let cases = [
        (
            CodecError::Underflow { needed: 7 },
            Error::Underflow { needed: 7 },
        ),
        (CodecError::VarIntTooLong, Error::VarIntTooLong),
        (CodecError::BadUtf8, Error::BadUtf8),
        (CodecError::BadQoS(3), Error::BadQoS(3)),
        // Everything else keeps its 3.1.1 message as the malformed reason.
        (
            CodecError::EmptySubscription,
            Error::Malformed("SUBSCRIBE/UNSUBSCRIBE must carry at least one topic filter"),
        ),
        (
            CodecError::BadProtocolLevel(3),
            Error::Malformed("unsupported protocol level 3 (only 4 = MQTT 3.1.1 is supported)"),
        ),
        (
            CodecError::BadProtocolName("MQIsdp".to_owned()),
            Error::Malformed("unsupported protocol name \"MQIsdp\""),
        ),
        (
            CodecError::UnknownPacketType(15),
            Error::Malformed("unknown packet type byte 0x0f"),
        ),
        (
            CodecError::MissingPacketId,
            Error::Malformed("PUBLISH with QoS > 0 missing packet id"),
        ),
        (
            CodecError::UnexpectedPayload,
            Error::Malformed("control packet expected an empty body"),
        ),
    ];
    for (from, into) in cases {
        assert_eq!(Error::from(from), into);
    }
}
