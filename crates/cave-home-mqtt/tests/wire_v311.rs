//! Pinning tests for the MQTT 3.1.1 wire codec.
//!
//! Every frame below is written out by hand from the OASIS MQTT 3.1.1
//! specification (§1.5.3 strings, §2.2 fixed header, §2.2.3 Remaining
//! Length, §3.1-§3.14 control packets) and compared byte for byte, so a
//! refactor of the codec cannot change what goes on the wire or which
//! error a malformed frame produces.

use bytes::{Bytes, BytesMut};
use cave_home_mqtt::codec::{decode_var_int, encode_var_int};
use cave_home_mqtt::{
    CodecError, ConnAck, ConnAckReturnCode, Connect, Packet, PacketType, PubAck, PubComp, PubRec,
    PubRel, Publish, QoS, SubAck, SubAckReturnCode, Subscribe, Subscription, UnsubAck, Unsubscribe,
    decode_packet, encode_packet,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// `packet` must encode to exactly `frame`, and `frame` must decode back to
/// `packet`, consuming every byte.
fn assert_frame(packet: &Packet, frame: &[u8]) -> TestResult {
    let encoded = encode_packet(packet)?;
    assert_eq!(&encoded[..], frame, "encoding of {packet:?}");
    let (decoded, used) = decode_packet(frame)?;
    assert_eq!(&decoded, packet, "decoding of {frame:02x?}");
    assert_eq!(used, frame.len(), "bytes consumed from {frame:02x?}");
    Ok(())
}

/// Each `(frame, error)` pair must fail to decode with exactly that error.
fn assert_rejected(cases: &[(&[u8], CodecError)]) {
    for (frame, expected) in cases {
        assert_eq!(
            decode_packet(frame).err().as_ref(),
            Some(expected),
            "frame {frame:02x?}"
        );
    }
}

fn publish(topic: &str, qos: QoS, flags: (bool, bool), packet_id: Option<u16>) -> Publish {
    let (dup, retain) = flags;
    Publish {
        topic: topic.to_owned(),
        qos,
        retain,
        dup,
        packet_id,
        payload: Bytes::from_static(b"ON"),
    }
}

// ---- §2.2.3 Remaining Length ----------------------------------------------

/// `(value, encoding)` at every boundary of the 1-4 byte forms.
const VAR_INT_BOUNDARIES: &[(u32, &[u8])] = &[
    (0, &[0x00]),
    (1, &[0x01]),
    (127, &[0x7f]),
    (128, &[0x80, 0x01]),
    (16_383, &[0xff, 0x7f]),
    (16_384, &[0x80, 0x80, 0x01]),
    (2_097_151, &[0xff, 0xff, 0x7f]),
    (2_097_152, &[0x80, 0x80, 0x80, 0x01]),
    (268_435_455, &[0xff, 0xff, 0xff, 0x7f]),
];

#[test]
fn var_int_encodes_to_the_spec_bytes_at_every_boundary() -> TestResult {
    for &(value, bytes) in VAR_INT_BOUNDARIES {
        let mut buf = BytesMut::new();
        encode_var_int(value, &mut buf)?;
        assert_eq!(&buf[..], bytes, "encoding of {value}");
    }
    Ok(())
}

#[test]
fn var_int_decodes_the_spec_bytes_at_every_boundary() -> TestResult {
    for &(value, bytes) in VAR_INT_BOUNDARIES {
        assert_eq!(decode_var_int(bytes)?, (value, bytes.len()), "{bytes:02x?}");
    }
    Ok(())
}

#[test]
fn var_int_encoder_appends_to_the_buffer() -> TestResult {
    let mut buf = BytesMut::from(&[0xaa][..]);
    encode_var_int(128, &mut buf)?;
    assert_eq!(&buf[..], [0xaa, 0x80, 0x01]);
    Ok(())
}

#[test]
fn var_int_encoder_rejects_values_beyond_four_bytes_after_writing_four() {
    // 268_435_456 needs a fifth byte: the encoder reports the error after
    // the four continuation bytes are already in the buffer.
    let mut buf = BytesMut::new();
    assert_eq!(
        encode_var_int(268_435_456, &mut buf),
        Err(CodecError::VarIntTooLong)
    );
    assert_eq!(&buf[..], [0x80, 0x80, 0x80, 0x80]);

    let mut buf = BytesMut::new();
    assert_eq!(
        encode_var_int(u32::MAX, &mut buf),
        Err(CodecError::VarIntTooLong)
    );
    assert_eq!(&buf[..], [0xff, 0xff, 0xff, 0xff]);
}

#[test]
fn var_int_decoder_stops_at_the_first_byte_without_continuation() -> TestResult {
    // Trailing bytes are left alone.
    assert_eq!(decode_var_int(&[0x05, 0xff, 0xff])?, (5, 1));
    assert_eq!(decode_var_int(&[0xff, 0x7f, 0x80])?, (16_383, 2));
    // Non-minimal encodings are accepted as they are.
    assert_eq!(decode_var_int(&[0x80, 0x00])?, (0, 2));
    assert_eq!(decode_var_int(&[0x81, 0x80, 0x80, 0x00])?, (1, 4));
    Ok(())
}

#[test]
fn var_int_decoder_reports_how_many_bytes_could_still_be_missing() {
    let cases: &[(&[u8], usize)] = &[
        (&[], 4),
        (&[0x80], 3),
        (&[0xff, 0xff], 2),
        (&[0x80, 0x80, 0x80], 1),
    ];
    for &(bytes, needed) in cases {
        assert_eq!(
            decode_var_int(bytes),
            Err(CodecError::Underflow { needed }),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn var_int_decoder_rejects_a_fourth_continuation_bit() {
    let cases: &[&[u8]] = &[
        &[0x80, 0x80, 0x80, 0x80],
        &[0x80, 0x80, 0x80, 0x80, 0x01],
        &[0xff, 0xff, 0xff, 0xff, 0x7f],
    ];
    for bytes in cases {
        assert_eq!(
            decode_var_int(bytes),
            Err(CodecError::VarIntTooLong),
            "{bytes:02x?}"
        );
    }
}

// ---- §3.1 CONNECT / §3.2 CONNACK --------------------------------------------

#[test]
fn connect_frames_are_byte_exact() -> TestResult {
    let mut frame = vec![
        0x10, 0x16, // CONNECT, remaining length 22
        0x00, 0x04, b'M', b'Q', b'T', b'T', // protocol name
        0x04, // protocol level 4
        0x02, // connect flags: clean session
        0x00, 0x3c, // keep alive 60 s
        0x00, 0x0a, // client id length 10
    ];
    frame.extend_from_slice(b"kitchen-pi");
    assert_frame(
        &Packet::Connect(Connect {
            client_id: "kitchen-pi".to_owned(),
            clean_session: true,
            keep_alive_secs: 60,
        }),
        &frame,
    )?;

    // Persistent session, empty client id, the largest keep alive.
    assert_frame(
        &Packet::Connect(Connect {
            client_id: String::new(),
            clean_session: false,
            keep_alive_secs: u16::MAX,
        }),
        &[
            0x10, 0x0c, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x04, 0x00, 0xff, 0xff, 0x00, 0x00,
        ],
    )
}

#[test]
fn connect_decoder_reads_only_clean_session_keep_alive_and_client_id() -> TestResult {
    // Will / user name / password flags and the payload fields after the
    // client id are not modelled by the 3.1.1 packet type: they are skipped.
    let frame = [
        0x10, 0x12, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x04, 0xc6, 0x00, 0x0a, 0x00, 0x01, b'c',
        0x00, 0x01, b'u', 0x00, 0x00,
    ];
    let (packet, used) = decode_packet(&frame)?;
    assert_eq!(
        packet,
        Packet::Connect(Connect {
            client_id: "c".to_owned(),
            clean_session: true,
            keep_alive_secs: 10,
        })
    );
    assert_eq!(used, frame.len());
    Ok(())
}

#[test]
fn connect_decoder_rejects_other_protocols() {
    assert_rejected(&[
        // MQTT 3.1 called itself "MQIsdp".
        (
            &[
                0x10, 0x0e, 0x00, 0x06, b'M', b'Q', b'I', b's', b'd', b'p', 0x03, 0x02, 0x00, 0x3c,
                0x00, 0x00,
            ],
            CodecError::BadProtocolName("MQIsdp".to_owned()),
        ),
        // Protocol level 5 belongs to the v5 codec.
        (
            &[
                0x10, 0x0c, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x05, 0x02, 0x00, 0x3c, 0x00, 0x00,
            ],
            CodecError::BadProtocolLevel(5),
        ),
        (
            &[
                0x10, 0x0c, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x03, 0x02, 0x00, 0x3c, 0x00, 0x00,
            ],
            CodecError::BadProtocolLevel(3),
        ),
        // The variable header stops after the protocol name.
        (
            &[0x10, 0x08, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x04, 0x02],
            CodecError::Underflow { needed: 2 },
        ),
        // No client id field at all.
        (
            &[
                0x10, 0x0a, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x04, 0x02, 0x00, 0x3c,
            ],
            CodecError::Underflow { needed: 2 },
        ),
    ]);
}

#[test]
fn connack_frames_are_byte_exact_for_every_return_code() -> TestResult {
    let codes = [
        (ConnAckReturnCode::Accepted, 0x00),
        (ConnAckReturnCode::UnacceptableProtocolVersion, 0x01),
        (ConnAckReturnCode::IdentifierRejected, 0x02),
        (ConnAckReturnCode::ServerUnavailable, 0x03),
        (ConnAckReturnCode::BadUsernameOrPassword, 0x04),
        (ConnAckReturnCode::NotAuthorized, 0x05),
    ];
    for (return_code, byte) in codes {
        for (session_present, flags) in [(false, 0x00), (true, 0x01)] {
            assert_frame(
                &Packet::ConnAck(ConnAck {
                    session_present,
                    return_code,
                }),
                &[0x20, 0x02, flags, byte],
            )?;
        }
    }
    Ok(())
}

#[test]
fn connack_decoder_reads_only_bit_zero_of_the_acknowledge_flags() -> TestResult {
    let present = |flags: u8| -> Result<bool, CodecError> {
        match decode_packet(&[0x20, 0x02, flags, 0x00])? {
            (Packet::ConnAck(ack), _) => Ok(ack.session_present),
            (other, _) => Err(CodecError::UnsupportedInPhase1(other.packet_type())),
        }
    };
    assert!(present(0xff)?);
    assert!(!present(0xfe)?);
    Ok(())
}

#[test]
fn connack_decoder_rejects_reserved_codes_and_short_bodies() {
    assert_rejected(&[
        (&[0x20, 0x02, 0x00, 0x06], CodecError::BadConnAckCode(6)),
        (&[0x20, 0x02, 0x00, 0xff], CodecError::BadConnAckCode(255)),
        (&[0x20, 0x01, 0x00], CodecError::Underflow { needed: 1 }),
        (&[0x20, 0x00], CodecError::Underflow { needed: 2 }),
    ]);
}

// ---- §3.3 PUBLISH -------------------------------------------------------------

#[test]
fn publish_frames_are_byte_exact_for_every_flag_combination() -> TestResult {
    // QoS 0: no packet identifier.
    assert_frame(
        &Packet::Publish(publish("a/b", QoS::AtMostOnce, (false, false), None)),
        &[0x30, 0x07, 0x00, 0x03, b'a', b'/', b'b', b'O', b'N'],
    )?;
    assert_frame(
        &Packet::Publish(publish("a/b", QoS::AtMostOnce, (false, true), None)),
        &[0x31, 0x07, 0x00, 0x03, b'a', b'/', b'b', b'O', b'N'],
    )?;
    // QoS 1 and 2: the packet identifier follows the topic.
    assert_frame(
        &Packet::Publish(publish(
            "a/b",
            QoS::AtLeastOnce,
            (false, false),
            Some(0x4242),
        )),
        &[
            0x32, 0x09, 0x00, 0x03, b'a', b'/', b'b', 0x42, 0x42, b'O', b'N',
        ],
    )?;
    assert_frame(
        &Packet::Publish(publish("a/b", QoS::AtLeastOnce, (true, false), Some(1))),
        &[
            0x3a, 0x09, 0x00, 0x03, b'a', b'/', b'b', 0x00, 0x01, b'O', b'N',
        ],
    )?;
    assert_frame(
        &Packet::Publish(publish("a/b", QoS::ExactlyOnce, (false, true), Some(2))),
        &[
            0x35, 0x09, 0x00, 0x03, b'a', b'/', b'b', 0x00, 0x02, b'O', b'N',
        ],
    )?;
    assert_frame(
        &Packet::Publish(publish("a/b", QoS::ExactlyOnce, (true, true), Some(0xffff))),
        &[
            0x3d, 0x09, 0x00, 0x03, b'a', b'/', b'b', 0xff, 0xff, b'O', b'N',
        ],
    )
}

#[test]
fn publish_with_empty_topic_and_empty_payload_is_four_bytes() -> TestResult {
    assert_frame(
        &Packet::Publish(Publish {
            topic: String::new(),
            qos: QoS::AtMostOnce,
            retain: false,
            dup: false,
            packet_id: None,
            payload: Bytes::new(),
        }),
        &[0x30, 0x02, 0x00, 0x00],
    )
}

#[test]
fn publish_encoder_needs_a_packet_id_above_qos0_and_drops_it_at_qos0() -> TestResult {
    for qos in [QoS::AtLeastOnce, QoS::ExactlyOnce] {
        let packet = Packet::Publish(publish("a/b", qos, (false, false), None));
        assert_eq!(encode_packet(&packet), Err(CodecError::MissingPacketId));
    }
    // A stray identifier on a QoS 0 PUBLISH never reaches the wire.
    let encoded = encode_packet(&Packet::Publish(publish(
        "a/b",
        QoS::AtMostOnce,
        (false, false),
        Some(7),
    )))?;
    assert_eq!(
        &encoded[..],
        [0x30, 0x07, 0x00, 0x03, b'a', b'/', b'b', b'O', b'N']
    );
    Ok(())
}

#[test]
fn publish_decoder_accepts_dup_on_qos0() -> TestResult {
    // The 3.1.1 decoder does not police DUP against the QoS level.
    let (packet, _) = decode_packet(&[0x38, 0x03, 0x00, 0x01, b't'])?;
    assert_eq!(
        packet,
        Packet::Publish(Publish {
            topic: "t".to_owned(),
            qos: QoS::AtMostOnce,
            retain: false,
            dup: true,
            packet_id: None,
            payload: Bytes::new(),
        })
    );
    Ok(())
}

#[test]
fn publish_decoder_rejects_malformed_frames() {
    assert_rejected(&[
        // Both QoS bits set.
        (&[0x36, 0x03, 0x00, 0x01, b't'], CodecError::BadQoS(3)),
        (&[0x3f, 0x03, 0x00, 0x01, b't'], CodecError::BadQoS(3)),
        // QoS 1 without room for the packet identifier.
        (
            &[0x32, 0x03, 0x00, 0x01, b't'],
            CodecError::Underflow { needed: 2 },
        ),
        (
            &[0x32, 0x04, 0x00, 0x01, b't', 0x00],
            CodecError::Underflow { needed: 1 },
        ),
        // Topic length prefix cut short, then a topic longer than the body.
        (&[0x30, 0x01, 0x00], CodecError::Underflow { needed: 1 }),
        (&[0x30, 0x00], CodecError::Underflow { needed: 2 }),
        (
            &[0x30, 0x03, 0x00, 0x05, b't'],
            CodecError::Underflow { needed: 4 },
        ),
        // 0xff is never valid UTF-8.
        (&[0x30, 0x03, 0x00, 0x01, 0xff], CodecError::BadUtf8),
    ]);
}

// ---- §1.5.3 strings at their limits -------------------------------------------

#[test]
fn a_65535_byte_topic_round_trips() -> TestResult {
    let topic = "t".repeat(65_535);
    // Remaining length 65_537 = 0x81 0x80 0x04.
    let mut frame = vec![0x30, 0x81, 0x80, 0x04, 0xff, 0xff];
    frame.extend_from_slice(topic.as_bytes());
    assert_frame(
        &Packet::Publish(Publish {
            topic,
            qos: QoS::AtMostOnce,
            retain: false,
            dup: false,
            packet_id: None,
            payload: Bytes::new(),
        }),
        &frame,
    )
}

#[test]
fn a_65536_byte_topic_is_written_whole_behind_a_saturated_length_prefix() -> TestResult {
    // The 3.1.1 encoder does not reject an over-long string: the two-byte
    // length saturates at 65_535 while every byte is still written, so the
    // last topic byte is read back as the first payload byte.
    let topic = "t".repeat(65_536);
    let encoded = encode_packet(&Packet::Publish(Publish {
        topic,
        qos: QoS::AtMostOnce,
        retain: false,
        dup: false,
        packet_id: None,
        payload: Bytes::from_static(b"!"),
    }))?;
    // Remaining length 65_539 = 0x83 0x80 0x04.
    assert_eq!(&encoded[..6], [0x30, 0x83, 0x80, 0x04, 0xff, 0xff]);
    assert_eq!(encoded.len(), 4 + 2 + 65_536 + 1);

    let (decoded, used) = decode_packet(&encoded)?;
    assert_eq!(used, encoded.len());
    assert_eq!(
        decoded,
        Packet::Publish(Publish {
            topic: "t".repeat(65_535),
            qos: QoS::AtMostOnce,
            retain: false,
            dup: false,
            packet_id: None,
            payload: Bytes::from_static(b"t!"),
        })
    );
    Ok(())
}

#[test]
fn strings_keep_multi_byte_utf8_and_the_null_character() -> TestResult {
    // "é" is two bytes, "€" three; U+0000 is not filtered by the codec.
    assert_frame(
        &Packet::Unsubscribe(Unsubscribe {
            packet_id: 1,
            topic_filters: vec!["é€".to_owned(), "a\0b".to_owned()],
        }),
        &[
            0xa2, 0x0e, 0x00, 0x01, 0x00, 0x05, 0xc3, 0xa9, 0xe2, 0x82, 0xac, 0x00, 0x03, b'a',
            0x00, b'b',
        ],
    )
}

// ---- §3.4-§3.7 PUBACK / PUBREC / PUBREL / PUBCOMP -----------------------------

#[test]
fn publish_acknowledgements_are_byte_exact() -> TestResult {
    assert_frame(
        &Packet::PubAck(PubAck { packet_id: 0x1234 }),
        &[0x40, 0x02, 0x12, 0x34],
    )?;
    assert_frame(
        &Packet::PubRec(PubRec { packet_id: 0x1234 }),
        &[0x50, 0x02, 0x12, 0x34],
    )?;
    // §3.6.1: PUBREL carries the fixed-header flags 0b0010.
    assert_frame(
        &Packet::PubRel(PubRel { packet_id: 0x1234 }),
        &[0x62, 0x02, 0x12, 0x34],
    )?;
    assert_frame(
        &Packet::PubComp(PubComp { packet_id: 0xffff }),
        &[0x70, 0x02, 0xff, 0xff],
    )?;
    assert_frame(
        &Packet::PubAck(PubAck { packet_id: 0 }),
        &[0x40, 0x02, 0x00, 0x00],
    )
}

#[test]
fn publish_acknowledgement_decoder_checks_flags_only_for_pubrel() -> TestResult {
    assert_rejected(&[
        (
            &[0x60, 0x02, 0x12, 0x34],
            CodecError::BadReservedFlags(0x00),
        ),
        (
            &[0x6f, 0x02, 0x12, 0x34],
            CodecError::BadReservedFlags(0x0f),
        ),
        // The flag check comes before the body is read.
        (&[0x60, 0x00], CodecError::BadReservedFlags(0x00)),
        (&[0x62, 0x01, 0x12], CodecError::Underflow { needed: 1 }),
        (&[0x40, 0x00], CodecError::Underflow { needed: 2 }),
        (&[0x50, 0x01, 0x12], CodecError::Underflow { needed: 1 }),
        (&[0x70, 0x00], CodecError::Underflow { needed: 2 }),
    ]);
    // Reserved flags on the other three, and bytes after the packet
    // identifier, are not looked at.
    assert_eq!(
        decode_packet(&[0x4f, 0x02, 0x12, 0x34])?,
        (Packet::PubAck(PubAck { packet_id: 0x1234 }), 4)
    );
    assert_eq!(
        decode_packet(&[0x51, 0x03, 0x12, 0x34, 0xff])?,
        (Packet::PubRec(PubRec { packet_id: 0x1234 }), 5)
    );
    assert_eq!(
        decode_packet(&[0x7a, 0x02, 0x12, 0x34])?,
        (Packet::PubComp(PubComp { packet_id: 0x1234 }), 4)
    );
    Ok(())
}

// ---- §3.8-§3.11 SUBSCRIBE / SUBACK / UNSUBSCRIBE / UNSUBACK -------------------

#[test]
fn subscribe_frame_is_byte_exact() -> TestResult {
    assert_frame(
        &Packet::Subscribe(Subscribe {
            packet_id: 10,
            subscriptions: vec![
                Subscription {
                    topic_filter: "a/b".to_owned(),
                    qos: QoS::AtLeastOnce,
                },
                Subscription {
                    topic_filter: "c/#".to_owned(),
                    qos: QoS::ExactlyOnce,
                },
                Subscription {
                    topic_filter: "+".to_owned(),
                    qos: QoS::AtMostOnce,
                },
            ],
        }),
        &[
            0x82, 0x12, 0x00, 0x0a, 0x00, 0x03, b'a', b'/', b'b', 0x01, 0x00, 0x03, b'c', b'/',
            b'#', 0x02, 0x00, 0x01, b'+', 0x00,
        ],
    )
}

#[test]
fn subscribe_decoder_rejects_malformed_frames() {
    assert_rejected(&[
        // §3.8.1: the fixed-header flags must be 0b0010.
        (
            &[0x80, 0x06, 0x00, 0x01, 0x00, 0x01, b'x', 0x00],
            CodecError::BadReservedFlags(0x00),
        ),
        (
            &[0x83, 0x06, 0x00, 0x01, 0x00, 0x01, b'x', 0x00],
            CodecError::BadReservedFlags(0x03),
        ),
        // §3.8.3: at least one topic filter.
        (&[0x82, 0x02, 0x00, 0x0a], CodecError::EmptySubscription),
        // Requested QoS 3, and reserved upper bits of the QoS byte.
        (
            &[0x82, 0x06, 0x00, 0x0a, 0x00, 0x01, b'x', 0x03],
            CodecError::BadQoS(3),
        ),
        (
            &[0x82, 0x06, 0x00, 0x0a, 0x00, 0x01, b'x', 0x81],
            CodecError::BadQoS(0x81),
        ),
        // The QoS byte of the last filter is missing.
        (
            &[0x82, 0x05, 0x00, 0x0a, 0x00, 0x01, b'x'],
            CodecError::Underflow { needed: 1 },
        ),
        (&[0x82, 0x01, 0x00], CodecError::Underflow { needed: 1 }),
        (
            &[0x82, 0x05, 0x00, 0x0a, 0x00, 0x01, 0xff],
            CodecError::BadUtf8,
        ),
    ]);
}

#[test]
fn suback_frames_are_byte_exact_for_every_return_code() -> TestResult {
    assert_frame(
        &Packet::SubAck(SubAck {
            packet_id: 10,
            return_codes: vec![
                SubAckReturnCode::MaxQoS0,
                SubAckReturnCode::MaxQoS1,
                SubAckReturnCode::MaxQoS2,
                SubAckReturnCode::Failure,
            ],
        }),
        &[0x90, 0x06, 0x00, 0x0a, 0x00, 0x01, 0x02, 0x80],
    )?;
    // A SUBACK without any return code is encoded and decoded as it is.
    assert_frame(
        &Packet::SubAck(SubAck {
            packet_id: 10,
            return_codes: vec![],
        }),
        &[0x90, 0x02, 0x00, 0x0a],
    )
}

#[test]
fn suback_decoder_rejects_unknown_return_codes() {
    assert_rejected(&[
        (
            &[0x90, 0x03, 0x00, 0x0a, 0x03],
            CodecError::BadSubAckCode(3),
        ),
        (
            &[0x90, 0x04, 0x00, 0x0a, 0x00, 0x81],
            CodecError::BadSubAckCode(0x81),
        ),
        (&[0x90, 0x01, 0x00], CodecError::Underflow { needed: 1 }),
    ]);
}

#[test]
fn unsubscribe_and_unsuback_frames_are_byte_exact() -> TestResult {
    assert_frame(
        &Packet::Unsubscribe(Unsubscribe {
            packet_id: 11,
            topic_filters: vec!["a/b".to_owned(), "c/d".to_owned()],
        }),
        &[
            0xa2, 0x0c, 0x00, 0x0b, 0x00, 0x03, b'a', b'/', b'b', 0x00, 0x03, b'c', b'/', b'd',
        ],
    )?;
    assert_frame(
        &Packet::UnsubAck(UnsubAck { packet_id: 11 }),
        &[0xb0, 0x02, 0x00, 0x0b],
    )
}

#[test]
fn unsubscribe_and_unsuback_decoders_reject_malformed_frames() {
    assert_rejected(&[
        // §3.10.1: the fixed-header flags must be 0b0010.
        (
            &[0xa0, 0x05, 0x00, 0x0b, 0x00, 0x01, b'x'],
            CodecError::BadReservedFlags(0x00),
        ),
        // §3.10.3: at least one topic filter.
        (&[0xa2, 0x02, 0x00, 0x0b], CodecError::EmptySubscription),
        (&[0xa2, 0x00], CodecError::Underflow { needed: 2 }),
        (
            &[0xa2, 0x04, 0x00, 0x0b, 0x00, 0x02],
            CodecError::Underflow { needed: 2 },
        ),
        (&[0xb0, 0x01, 0x00], CodecError::Underflow { needed: 1 }),
    ]);
}

// ---- §3.12-§3.14 PINGREQ / PINGRESP / DISCONNECT ------------------------------

#[test]
fn bodiless_packets_are_two_bytes() -> TestResult {
    assert_frame(&Packet::PingReq, &[0xc0, 0x00])?;
    assert_frame(&Packet::PingResp, &[0xd0, 0x00])?;
    assert_frame(&Packet::Disconnect, &[0xe0, 0x00])
}

#[test]
fn bodiless_packets_reject_a_body() {
    assert_rejected(&[
        (&[0xc0, 0x01, 0x00], CodecError::UnexpectedPayload),
        (&[0xd0, 0x01, 0x00], CodecError::UnexpectedPayload),
        (&[0xe0, 0x02, 0x00, 0x00], CodecError::UnexpectedPayload),
    ]);
}

// ---- §2.2 fixed header ----------------------------------------------------------

#[test]
fn fixed_header_errors_are_reported_before_the_body_is_read() {
    assert_rejected(&[
        (&[], CodecError::Underflow { needed: 2 }),
        // Types 0 and 15 are reserved in 3.1.1.
        (&[0x00, 0x00], CodecError::UnknownPacketType(0)),
        (&[0xf0, 0x00], CodecError::UnknownPacketType(15)),
        (&[0x0f], CodecError::UnknownPacketType(0)),
        // The Remaining Length is missing or incomplete.
        (&[0xc0], CodecError::Underflow { needed: 4 }),
        (&[0x30, 0x80], CodecError::Underflow { needed: 3 }),
        (
            &[0x30, 0x80, 0x80, 0x80],
            CodecError::Underflow { needed: 1 },
        ),
        (&[0x30, 0x80, 0x80, 0x80, 0x80], CodecError::VarIntTooLong),
        // The body is shorter than the Remaining Length says.
        (&[0x40, 0x02, 0x12], CodecError::Underflow { needed: 1 }),
        (&[0x30, 0x7f], CodecError::Underflow { needed: 127 }),
        (
            &[0x30, 0xff, 0xff, 0xff, 0x7f],
            CodecError::Underflow {
                needed: 268_435_455,
            },
        ),
    ]);
}

#[test]
fn decoder_consumes_exactly_one_frame() -> TestResult {
    let two = [0xc0, 0x00, 0x40, 0x02, 0x00, 0x07, 0xff];
    assert_eq!(decode_packet(&two)?, (Packet::PingReq, 2));
    assert_eq!(
        decode_packet(&two[2..])?,
        (Packet::PubAck(PubAck { packet_id: 7 }), 4)
    );
    Ok(())
}

#[test]
fn packet_type_nibbles_match_the_spec_table() {
    let table = [
        (1, PacketType::Connect),
        (2, PacketType::ConnAck),
        (3, PacketType::Publish),
        (4, PacketType::PubAck),
        (5, PacketType::PubRec),
        (6, PacketType::PubRel),
        (7, PacketType::PubComp),
        (8, PacketType::Subscribe),
        (9, PacketType::SubAck),
        (10, PacketType::Unsubscribe),
        (11, PacketType::UnsubAck),
        (12, PacketType::PingReq),
        (13, PacketType::PingResp),
        (14, PacketType::Disconnect),
    ];
    for byte in 0..=u8::MAX {
        let expected = table.iter().find(|(b, _)| *b == byte).map(|(_, t)| *t);
        assert_eq!(PacketType::from_u8(byte), expected, "nibble {byte}");
    }
    for (byte, packet_type) in table {
        assert_eq!(packet_type as u8, byte);
    }
}

#[test]
fn packets_report_their_own_type() {
    let id = 1;
    let cases = [
        (
            Packet::Connect(Connect {
                client_id: String::new(),
                clean_session: true,
                keep_alive_secs: 0,
            }),
            PacketType::Connect,
        ),
        (
            Packet::ConnAck(ConnAck {
                session_present: false,
                return_code: ConnAckReturnCode::Accepted,
            }),
            PacketType::ConnAck,
        ),
        (
            Packet::Publish(publish("t", QoS::AtMostOnce, (false, false), None)),
            PacketType::Publish,
        ),
        (Packet::PubAck(PubAck { packet_id: id }), PacketType::PubAck),
        (Packet::PubRec(PubRec { packet_id: id }), PacketType::PubRec),
        (Packet::PubRel(PubRel { packet_id: id }), PacketType::PubRel),
        (
            Packet::PubComp(PubComp { packet_id: id }),
            PacketType::PubComp,
        ),
        (
            Packet::Subscribe(Subscribe {
                packet_id: id,
                subscriptions: vec![],
            }),
            PacketType::Subscribe,
        ),
        (
            Packet::SubAck(SubAck {
                packet_id: id,
                return_codes: vec![],
            }),
            PacketType::SubAck,
        ),
        (
            Packet::Unsubscribe(Unsubscribe {
                packet_id: id,
                topic_filters: vec![],
            }),
            PacketType::Unsubscribe,
        ),
        (
            Packet::UnsubAck(UnsubAck { packet_id: id }),
            PacketType::UnsubAck,
        ),
        (Packet::PingReq, PacketType::PingReq),
        (Packet::PingResp, PacketType::PingResp),
        (Packet::Disconnect, PacketType::Disconnect),
    ];
    for (packet, packet_type) in cases {
        assert_eq!(packet.packet_type(), packet_type, "{packet:?}");
    }
}

#[test]
fn encoder_writes_empty_filter_lists_that_the_decoder_refuses() -> TestResult {
    // The encoder does not validate §3.8.3 / §3.10.3; the decoder does.
    let subscribe = encode_packet(&Packet::Subscribe(Subscribe {
        packet_id: 10,
        subscriptions: vec![],
    }))?;
    assert_eq!(&subscribe[..], [0x82, 0x02, 0x00, 0x0a]);
    assert_eq!(
        decode_packet(&subscribe),
        Err(CodecError::EmptySubscription)
    );
    let unsubscribe = encode_packet(&Packet::Unsubscribe(Unsubscribe {
        packet_id: 11,
        topic_filters: vec![],
    }))?;
    assert_eq!(&unsubscribe[..], [0xa2, 0x02, 0x00, 0x0b]);
    assert_eq!(
        decode_packet(&unsubscribe),
        Err(CodecError::EmptySubscription)
    );
    Ok(())
}

// ---- value tables -----------------------------------------------------------------

#[test]
fn qos_and_return_code_bytes_match_the_spec_tables() {
    for byte in 0..=u8::MAX {
        let qos = match byte {
            0 => Some(QoS::AtMostOnce),
            1 => Some(QoS::AtLeastOnce),
            2 => Some(QoS::ExactlyOnce),
            _ => None,
        };
        assert_eq!(QoS::from_u8(byte), qos, "QoS {byte}");

        let connack = match byte {
            0 => Some(ConnAckReturnCode::Accepted),
            1 => Some(ConnAckReturnCode::UnacceptableProtocolVersion),
            2 => Some(ConnAckReturnCode::IdentifierRejected),
            3 => Some(ConnAckReturnCode::ServerUnavailable),
            4 => Some(ConnAckReturnCode::BadUsernameOrPassword),
            5 => Some(ConnAckReturnCode::NotAuthorized),
            _ => None,
        };
        assert_eq!(ConnAckReturnCode::from_u8(byte), connack, "CONNACK {byte}");

        let suback = match byte {
            0x00 => Some(SubAckReturnCode::MaxQoS0),
            0x01 => Some(SubAckReturnCode::MaxQoS1),
            0x02 => Some(SubAckReturnCode::MaxQoS2),
            0x80 => Some(SubAckReturnCode::Failure),
            _ => None,
        };
        assert_eq!(SubAckReturnCode::from_u8(byte), suback, "SUBACK {byte}");
    }
}

// ---- error text ---------------------------------------------------------------------

#[test]
fn codec_error_messages_are_stable() {
    let cases = [
        (
            CodecError::Underflow { needed: 3 },
            "buffer underflow: need 3 more bytes",
        ),
        (
            CodecError::VarIntTooLong,
            "variable-length integer exceeds 4-byte maximum",
        ),
        (
            CodecError::UnknownPacketType(15),
            "unknown packet type byte 0x0f",
        ),
        (CodecError::BadQoS(3), "unknown QoS value 3"),
        (
            CodecError::BadProtocolName("MQIsdp".to_owned()),
            "unsupported protocol name \"MQIsdp\"",
        ),
        (
            CodecError::BadProtocolLevel(5),
            "unsupported protocol level 5 (only 4 = MQTT 3.1.1 is supported)",
        ),
        (
            CodecError::BadConnAckCode(6),
            "CONNACK return code 6 is reserved",
        ),
        (CodecError::BadUtf8, "UTF-8 string field malformed"),
        (
            CodecError::MissingPacketId,
            "PUBLISH with QoS > 0 missing packet id",
        ),
        (
            CodecError::UnsupportedInPhase1(PacketType::Publish),
            "packet type Publish is not yet supported by Phase 1 codec",
        ),
        (
            CodecError::BadReservedFlags(0x0f),
            "reserved fixed-header flags 0xf are invalid for this packet type",
        ),
        (
            CodecError::EmptySubscription,
            "SUBSCRIBE/UNSUBSCRIBE must carry at least one topic filter",
        ),
        (
            CodecError::BadSubAckCode(3),
            "SUBACK return code 3 is invalid",
        ),
        (
            CodecError::UnexpectedPayload,
            "control packet expected an empty body",
        ),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
}
