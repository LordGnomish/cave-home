//! MQTT 5.0 control-packet wire codec (§3) — clean-room from OASIS 5.0.

use crate::codec::decode_var_int;
use crate::packet::QoS;
use crate::v5::packet::{
    AuthV5, ConnAckV5, ConnectV5, DisconnectV5, PacketV5, PubAckV5, PubCompV5,
    PubRecV5, PubRelV5, PublishV5, RetainHandling, SubAckV5, SubscribeV5,
    SubscriptionV5, UnsubAckV5, UnsubscribeV5, Will,
};
use crate::v5::property::{decode_properties, encode_properties, Property};
use crate::v5::reason::ReasonCode;
use crate::v5::wire::{
    get_binary, get_string, get_u16, get_u8, put_binary, put_string, put_u16,
    put_u8, put_var_int, Error,
};
use bytes::{Bytes, BytesMut};

/// §3.6.1 / §3.8.1 / §3.10.1 — PUBREL, SUBSCRIBE and UNSUBSCRIBE carry
/// the fixed header flag nibble `0b0010`.
const RESERVED_0010: u8 = 0x02;

/// Encode an MQTT 5.0 control packet into a fresh buffer.
pub fn encode_v5(p: &PacketV5) -> Result<BytesMut, Error> {
    let mut body = BytesMut::new();
    let (ty, flags) = match p {
        PacketV5::Connect(c) => {
            encode_connect(c, &mut body)?;
            (1u8, 0u8)
        }
        PacketV5::ConnAck(c) => {
            encode_connack(c, &mut body)?;
            (2, 0)
        }
        PacketV5::Publish(p) => {
            let f = encode_publish(p, &mut body)?;
            (3, f)
        }
        PacketV5::PubAck(a) => {
            encode_ack(a.packet_id, a.reason_code, &a.properties, &mut body)?;
            (4, 0)
        }
        PacketV5::PubRec(a) => {
            encode_ack(a.packet_id, a.reason_code, &a.properties, &mut body)?;
            (5, 0)
        }
        PacketV5::PubRel(a) => {
            encode_ack(a.packet_id, a.reason_code, &a.properties, &mut body)?;
            (6, RESERVED_0010)
        }
        PacketV5::PubComp(a) => {
            encode_ack(a.packet_id, a.reason_code, &a.properties, &mut body)?;
            (7, 0)
        }
        PacketV5::Subscribe(s) => {
            encode_subscribe(s, &mut body)?;
            (8, RESERVED_0010)
        }
        PacketV5::SubAck(s) => {
            encode_id_props_codes(s.packet_id, &s.properties, &s.reason_codes, &mut body)?;
            (9, 0)
        }
        PacketV5::Unsubscribe(u) => {
            encode_unsubscribe(u, &mut body)?;
            (10, RESERVED_0010)
        }
        PacketV5::UnsubAck(u) => {
            encode_id_props_codes(u.packet_id, &u.properties, &u.reason_codes, &mut body)?;
            (11, 0)
        }
        PacketV5::PingReq => (12, 0),
        PacketV5::PingResp => (13, 0),
        PacketV5::Disconnect(d) => {
            encode_reason_props(d.reason_code, &d.properties, &mut body)?;
            (14, 0)
        }
        PacketV5::Auth(a) => {
            encode_reason_props(a.reason_code, &a.properties, &mut body)?;
            (15, 0)
        }
    };

    let mut out = BytesMut::with_capacity(body.len() + 5);
    put_u8((ty << 4) | (flags & 0x0f), &mut out);
    put_var_int(u32::try_from(body.len()).map_err(|_| Error::VarIntTooLong)?, &mut out)?;
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decode one MQTT 5.0 packet, returning it and the bytes consumed.
pub fn decode_v5(input: &[u8]) -> Result<(PacketV5, usize), Error> {
    if input.is_empty() {
        return Err(Error::Underflow { needed: 2 });
    }
    let header = input[0];
    let type_nibble = header >> 4;
    let flags = header & 0x0f;

    let (remaining, len_bytes) = decode_var_int(&input[1..])?;
    let total = 1 + len_bytes + remaining as usize;
    if input.len() < total {
        return Err(Error::Underflow { needed: total - input.len() });
    }
    let mut body = &input[1 + len_bytes..total];

    let packet = match type_nibble {
        1 => PacketV5::Connect(decode_connect(&mut body)?),
        2 => PacketV5::ConnAck(decode_connack(&mut body)?),
        3 => PacketV5::Publish(decode_publish(&mut body, flags)?),
        4 => {
            let (id, rc, props) = decode_ack(&mut body, "PUBACK")?;
            PacketV5::PubAck(PubAckV5 { packet_id: id, reason_code: rc, properties: props })
        }
        5 => {
            let (id, rc, props) = decode_ack(&mut body, "PUBREC")?;
            PacketV5::PubRec(PubRecV5 { packet_id: id, reason_code: rc, properties: props })
        }
        6 => {
            require_flags(flags, RESERVED_0010, "PUBREL")?;
            let (id, rc, props) = decode_ack(&mut body, "PUBREL")?;
            PacketV5::PubRel(PubRelV5 { packet_id: id, reason_code: rc, properties: props })
        }
        7 => {
            let (id, rc, props) = decode_ack(&mut body, "PUBCOMP")?;
            PacketV5::PubComp(PubCompV5 { packet_id: id, reason_code: rc, properties: props })
        }
        8 => {
            require_flags(flags, RESERVED_0010, "SUBSCRIBE")?;
            PacketV5::Subscribe(decode_subscribe(&mut body)?)
        }
        9 => PacketV5::SubAck(decode_suback(&mut body)?),
        10 => {
            require_flags(flags, RESERVED_0010, "UNSUBSCRIBE")?;
            PacketV5::Unsubscribe(decode_unsubscribe(&mut body)?)
        }
        11 => PacketV5::UnsubAck(decode_unsuback(&mut body)?),
        12 => PacketV5::PingReq,
        13 => PacketV5::PingResp,
        14 => {
            let (rc, props) = decode_reason_props(&mut body, "DISCONNECT")?;
            PacketV5::Disconnect(DisconnectV5 { reason_code: rc, properties: props })
        }
        15 => {
            let (rc, props) = decode_reason_props(&mut body, "AUTH")?;
            PacketV5::Auth(AuthV5 { reason_code: rc, properties: props })
        }
        other => return Err(Error::Malformed(unknown_type(other))),
    };
    Ok((packet, total))
}

fn unknown_type(_n: u8) -> &'static str {
    "unknown MQTT control packet type"
}

fn require_flags(actual: u8, expected: u8, _packet: &'static str) -> Result<(), Error> {
    if actual == expected {
        Ok(())
    } else {
        Err(Error::Malformed("reserved fixed-header flags must be 0b0010"))
    }
}

// ---- CONNECT / CONNACK ---------------------------------------------------

fn encode_connect(c: &ConnectV5, out: &mut BytesMut) -> Result<(), Error> {
    put_string("MQTT", out)?;
    put_u8(5, out); // §3.1.2.2 protocol level = 5
    let mut flags = 0u8;
    if c.username.is_some() {
        flags |= 0x80;
    }
    if c.password.is_some() {
        flags |= 0x40;
    }
    if let Some(w) = &c.will {
        flags |= 0x04;
        flags |= (w.qos as u8) << 3;
        if w.retain {
            flags |= 0x20;
        }
    }
    if c.clean_start {
        flags |= 0x02;
    }
    put_u8(flags, out);
    put_u16(c.keep_alive_secs, out);
    encode_properties(&c.properties, out)?;
    // Payload: client id, then will, then username, then password.
    put_string(&c.client_id, out)?;
    if let Some(w) = &c.will {
        encode_properties(&w.properties, out)?;
        put_string(&w.topic, out)?;
        put_binary(&w.payload, out)?;
    }
    if let Some(u) = &c.username {
        put_string(u, out)?;
    }
    if let Some(p) = &c.password {
        put_binary(p, out)?;
    }
    Ok(())
}

fn decode_connect(buf: &mut &[u8]) -> Result<ConnectV5, Error> {
    let name = get_string(buf)?;
    if name != "MQTT" {
        return Err(Error::BadProtocolName(name));
    }
    let level = get_u8(buf)?;
    if level != 5 {
        return Err(Error::BadProtocolLevel(level));
    }
    let flags = get_u8(buf)?;
    if flags & 0x01 != 0 {
        return Err(Error::Malformed("CONNECT reserved flag bit 0 must be 0"));
    }
    let keep_alive_secs = get_u16(buf)?;
    let properties = decode_properties(buf)?;
    let client_id = get_string(buf)?;

    let will = if flags & 0x04 != 0 {
        let will_properties = decode_properties(buf)?;
        let topic = get_string(buf)?;
        let payload = get_binary(buf)?;
        let qos = QoS::from_u8((flags >> 3) & 0x03).ok_or(Error::BadQoS((flags >> 3) & 0x03))?;
        Some(Will { topic, payload, qos, retain: flags & 0x20 != 0, properties: will_properties })
    } else {
        if (flags >> 3) & 0x03 != 0 || flags & 0x20 != 0 {
            return Err(Error::Malformed("Will QoS/Retain set without Will Flag"));
        }
        None
    };
    let username = if flags & 0x80 != 0 { Some(get_string(buf)?) } else { None };
    let password = if flags & 0x40 != 0 { Some(get_binary(buf)?) } else { None };

    Ok(ConnectV5 {
        client_id,
        clean_start: flags & 0x02 != 0,
        keep_alive_secs,
        properties,
        will,
        username,
        password,
    })
}

fn encode_connack(c: &ConnAckV5, out: &mut BytesMut) -> Result<(), Error> {
    put_u8(u8::from(c.session_present), out); // §3.2.2.1.1 only bit 0
    put_u8(c.reason_code as u8, out);
    encode_properties(&c.properties, out)
}

fn decode_connack(buf: &mut &[u8]) -> Result<ConnAckV5, Error> {
    let ack_flags = get_u8(buf)?;
    let code = get_u8(buf)?;
    let reason_code =
        ReasonCode::from_u8(code).ok_or(Error::BadReasonCode { packet: "CONNACK", code })?;
    let properties = decode_properties(buf)?;
    Ok(ConnAckV5 { session_present: ack_flags & 0x01 != 0, reason_code, properties })
}

// ---- PUBLISH -------------------------------------------------------------

fn encode_publish(p: &PublishV5, out: &mut BytesMut) -> Result<u8, Error> {
    put_string(&p.topic, out)?;
    if p.qos != QoS::AtMostOnce {
        let pid = p.packet_id.ok_or(Error::Malformed("PUBLISH QoS > 0 missing packet id"))?;
        put_u16(pid, out);
    }
    encode_properties(&p.properties, out)?;
    out.extend_from_slice(&p.payload);
    Ok((u8::from(p.dup) << 3) | ((p.qos as u8) << 1) | u8::from(p.retain))
}

fn decode_publish(buf: &mut &[u8], flags: u8) -> Result<PublishV5, Error> {
    let dup = flags & 0x08 != 0;
    let qos = QoS::from_u8((flags >> 1) & 0x03).ok_or(Error::BadQoS((flags >> 1) & 0x03))?;
    let retain = flags & 0x01 != 0;
    if dup && qos == QoS::AtMostOnce {
        return Err(Error::Malformed("PUBLISH DUP must be 0 for QoS 0 (§3.3.1.1)"));
    }
    let topic = get_string(buf)?;
    let packet_id = if qos == QoS::AtMostOnce { None } else { Some(get_u16(buf)?) };
    let properties = decode_properties(buf)?;
    let payload = Bytes::copy_from_slice(buf);
    *buf = &buf[buf.len()..];
    Ok(PublishV5 { topic, qos, retain, dup, packet_id, properties, payload })
}

// ---- PUBACK / PUBREC / PUBREL / PUBCOMP ----------------------------------

fn encode_ack(
    packet_id: u16,
    reason: ReasonCode,
    props: &[Property],
    out: &mut BytesMut,
) -> Result<(), Error> {
    put_u16(packet_id, out);
    // §3.4.2.1: Success with no properties uses the compact 2-byte form.
    if reason == ReasonCode::Success && props.is_empty() {
        return Ok(());
    }
    put_u8(reason as u8, out);
    encode_properties(props, out)
}

fn decode_ack(
    buf: &mut &[u8],
    packet: &'static str,
) -> Result<(u16, ReasonCode, Vec<Property>), Error> {
    let packet_id = get_u16(buf)?;
    if buf.is_empty() {
        return Ok((packet_id, ReasonCode::Success, Vec::new()));
    }
    let code = get_u8(buf)?;
    let reason = ReasonCode::from_u8(code).ok_or(Error::BadReasonCode { packet, code })?;
    let properties = if buf.is_empty() { Vec::new() } else { decode_properties(buf)? };
    Ok((packet_id, reason, properties))
}

// ---- SUBSCRIBE / SUBACK / UNSUBSCRIBE / UNSUBACK -------------------------

fn encode_subscribe(s: &SubscribeV5, out: &mut BytesMut) -> Result<(), Error> {
    put_u16(s.packet_id, out);
    encode_properties(&s.properties, out)?;
    for sub in &s.subscriptions {
        put_string(&sub.topic_filter, out)?;
        let mut opt = sub.qos as u8;
        if sub.no_local {
            opt |= 0x04;
        }
        if sub.retain_as_published {
            opt |= 0x08;
        }
        opt |= (sub.retain_handling as u8) << 4;
        put_u8(opt, out);
    }
    Ok(())
}

fn decode_subscribe(buf: &mut &[u8]) -> Result<SubscribeV5, Error> {
    let packet_id = get_u16(buf)?;
    let properties = decode_properties(buf)?;
    let mut subscriptions = Vec::new();
    while !buf.is_empty() {
        let topic_filter = get_string(buf)?;
        let opt = get_u8(buf)?;
        if opt & 0xC0 != 0 {
            return Err(Error::Malformed("subscription options reserved bits 6-7 must be 0"));
        }
        let qos = QoS::from_u8(opt & 0x03).ok_or(Error::BadQoS(opt & 0x03))?;
        let retain_handling = RetainHandling::from_u8((opt >> 4) & 0x03)
            .ok_or(Error::Malformed("invalid Retain Handling value 3 (§3.8.3.1)"))?;
        subscriptions.push(SubscriptionV5 {
            topic_filter,
            qos,
            no_local: opt & 0x04 != 0,
            retain_as_published: opt & 0x08 != 0,
            retain_handling,
        });
    }
    if subscriptions.is_empty() {
        return Err(Error::Malformed("SUBSCRIBE must contain at least one topic filter"));
    }
    Ok(SubscribeV5 { packet_id, properties, subscriptions })
}

fn encode_id_props_codes(
    packet_id: u16,
    props: &[Property],
    codes: &[ReasonCode],
    out: &mut BytesMut,
) -> Result<(), Error> {
    put_u16(packet_id, out);
    encode_properties(props, out)?;
    for rc in codes {
        put_u8(*rc as u8, out);
    }
    Ok(())
}

fn decode_suback(buf: &mut &[u8]) -> Result<SubAckV5, Error> {
    let (packet_id, properties, reason_codes) = decode_id_props_codes(buf, "SUBACK")?;
    Ok(SubAckV5 { packet_id, properties, reason_codes })
}

fn decode_unsuback(buf: &mut &[u8]) -> Result<UnsubAckV5, Error> {
    let (packet_id, properties, reason_codes) = decode_id_props_codes(buf, "UNSUBACK")?;
    Ok(UnsubAckV5 { packet_id, properties, reason_codes })
}

fn decode_id_props_codes(
    buf: &mut &[u8],
    packet: &'static str,
) -> Result<(u16, Vec<Property>, Vec<ReasonCode>), Error> {
    let packet_id = get_u16(buf)?;
    let properties = decode_properties(buf)?;
    let mut reason_codes = Vec::new();
    while !buf.is_empty() {
        let code = get_u8(buf)?;
        reason_codes.push(
            ReasonCode::from_u8(code).ok_or(Error::BadReasonCode { packet, code })?,
        );
    }
    Ok((packet_id, properties, reason_codes))
}

fn encode_unsubscribe(u: &UnsubscribeV5, out: &mut BytesMut) -> Result<(), Error> {
    put_u16(u.packet_id, out);
    encode_properties(&u.properties, out)?;
    for f in &u.topic_filters {
        put_string(f, out)?;
    }
    Ok(())
}

fn decode_unsubscribe(buf: &mut &[u8]) -> Result<UnsubscribeV5, Error> {
    let packet_id = get_u16(buf)?;
    let properties = decode_properties(buf)?;
    let mut topic_filters = Vec::new();
    while !buf.is_empty() {
        topic_filters.push(get_string(buf)?);
    }
    if topic_filters.is_empty() {
        return Err(Error::Malformed("UNSUBSCRIBE must contain at least one topic filter"));
    }
    Ok(UnsubscribeV5 { packet_id, properties, topic_filters })
}

// ---- DISCONNECT / AUTH ---------------------------------------------------

fn encode_reason_props(
    reason: ReasonCode,
    props: &[Property],
    out: &mut BytesMut,
) -> Result<(), Error> {
    // §3.14.2.1 / §3.15.2.1: Success + no properties ⇒ empty body.
    if reason == ReasonCode::Success && props.is_empty() {
        return Ok(());
    }
    put_u8(reason as u8, out);
    encode_properties(props, out)
}

fn decode_reason_props(
    buf: &mut &[u8],
    packet: &'static str,
) -> Result<(ReasonCode, Vec<Property>), Error> {
    if buf.is_empty() {
        return Ok((ReasonCode::Success, Vec::new()));
    }
    let code = get_u8(buf)?;
    let reason = ReasonCode::from_u8(code).ok_or(Error::BadReasonCode { packet, code })?;
    let properties = if buf.is_empty() { Vec::new() } else { decode_properties(buf)? };
    Ok((reason, properties))
}

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
