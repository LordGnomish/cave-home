//! MQTT 3.1.1 wire codec — clean-room from OASIS §2.2 fixed header,
//! §2.2.3 variable-length integer, and §3.1/§3.2/§3.3 packet bodies.

use crate::packet::{
    ConnAck, ConnAckReturnCode, Connect, Packet, PacketType, PubAck, PubComp,
    PubRec, PubRel, Publish, QoS, SubAck, SubAckReturnCode, Subscribe,
    Subscription, UnsubAck, Unsubscribe,
};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CodecError {
    #[error("buffer underflow: need {needed} more bytes")]
    Underflow { needed: usize },
    #[error("variable-length integer exceeds 4-byte maximum")]
    VarIntTooLong,
    #[error("unknown packet type byte 0x{0:02x}")]
    UnknownPacketType(u8),
    #[error("unknown QoS value {0}")]
    BadQoS(u8),
    #[error("unsupported protocol name {0:?}")]
    BadProtocolName(String),
    #[error("unsupported protocol level {0} (only 4 = MQTT 3.1.1 is supported)")]
    BadProtocolLevel(u8),
    #[error("CONNACK return code {0} is reserved")]
    BadConnAckCode(u8),
    #[error("UTF-8 string field malformed")]
    BadUtf8,
    #[error("PUBLISH with QoS > 0 missing packet id")]
    MissingPacketId,
    #[error("packet type {0:?} is not yet supported by Phase 1 codec")]
    UnsupportedInPhase1(PacketType),
    #[error("reserved fixed-header flags 0x{0:x} are invalid for this packet type")]
    BadReservedFlags(u8),
    #[error("SUBSCRIBE/UNSUBSCRIBE must carry at least one topic filter")]
    EmptySubscription,
    #[error("SUBACK return code {0} is invalid")]
    BadSubAckCode(u8),
    #[error("control packet expected an empty body")]
    UnexpectedPayload,
}

/// MQTT 3.1.1 §2.2.3 — Remaining-Length variable-byte integer encoder.
pub fn encode_var_int(value: u32, buf: &mut BytesMut) -> Result<(), CodecError> {
    let mut x = value;
    let mut written = 0;
    loop {
        let mut byte = (x & 0x7f) as u8;
        x >>= 7;
        if x > 0 {
            byte |= 0x80;
        }
        buf.put_u8(byte);
        written += 1;
        if x == 0 {
            return Ok(());
        }
        if written == 4 {
            return Err(CodecError::VarIntTooLong);
        }
    }
}

/// MQTT 3.1.1 §2.2.3 — Remaining-Length variable-byte integer decoder.
/// Returns `(value, bytes_consumed)`.
pub fn decode_var_int(input: &[u8]) -> Result<(u32, usize), CodecError> {
    let mut value: u32 = 0;
    let mut multiplier: u32 = 1;
    for (i, &byte) in input.iter().enumerate().take(4) {
        value += u32::from(byte & 0x7f) * multiplier;
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
        multiplier *= 128;
    }
    if input.len() < 4 {
        Err(CodecError::Underflow { needed: 4 - input.len() })
    } else {
        Err(CodecError::VarIntTooLong)
    }
}

/// MQTT 3.1.1 §1.5.3 — UTF-8 prefixed string (2-byte length + bytes).
fn encode_str(s: &str, buf: &mut BytesMut) {
    buf.put_u16(u16::try_from(s.len()).unwrap_or(u16::MAX));
    buf.put_slice(s.as_bytes());
}

fn decode_str(buf: &mut &[u8]) -> Result<String, CodecError> {
    if buf.len() < 2 {
        return Err(CodecError::Underflow { needed: 2 - buf.len() });
    }
    let len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
    buf.advance(2);
    if buf.len() < len {
        return Err(CodecError::Underflow { needed: len - buf.len() });
    }
    let s = std::str::from_utf8(&buf[..len])
        .map_err(|_| CodecError::BadUtf8)?
        .to_owned();
    buf.advance(len);
    Ok(s)
}

pub fn encode_packet(packet: &Packet) -> Result<BytesMut, CodecError> {
    let mut body = BytesMut::new();
    let (header_flags, packet_type) = match packet {
        Packet::Connect(c) => {
            encode_connect(c, &mut body);
            (0u8, PacketType::Connect)
        }
        Packet::ConnAck(c) => {
            encode_connack(c, &mut body);
            (0u8, PacketType::ConnAck)
        }
        Packet::Publish(p) => {
            let flags = encode_publish(p, &mut body)?;
            (flags, PacketType::Publish)
        }
        Packet::Subscribe(s) => {
            let flags = encode_subscribe(s, &mut body);
            (flags, PacketType::Subscribe)
        }
        Packet::SubAck(a) => {
            encode_suback(a, &mut body);
            (0u8, PacketType::SubAck)
        }
        Packet::Unsubscribe(u) => {
            let flags = encode_unsubscribe(u, &mut body);
            (flags, PacketType::Unsubscribe)
        }
        Packet::UnsubAck(a) => {
            encode_unsuback(a, &mut body);
            (0u8, PacketType::UnsubAck)
        }
        Packet::PingReq => (0u8, PacketType::PingReq),
        Packet::PingResp => (0u8, PacketType::PingResp),
        Packet::Disconnect => (0u8, PacketType::Disconnect),
        Packet::PubAck(p) => {
            body.put_u16(p.packet_id);
            (0u8, PacketType::PubAck)
        }
        Packet::PubRec(p) => {
            body.put_u16(p.packet_id);
            (0u8, PacketType::PubRec)
        }
        Packet::PubRel(p) => {
            // §3.6.1: PUBREL reserved fixed-header flags are 0b0010.
            body.put_u16(p.packet_id);
            (0x02, PacketType::PubRel)
        }
        Packet::PubComp(p) => {
            body.put_u16(p.packet_id);
            (0u8, PacketType::PubComp)
        }
    };

    let mut out = BytesMut::with_capacity(body.len() + 5);
    out.put_u8(((packet_type as u8) << 4) | (header_flags & 0x0f));
    encode_var_int(u32::try_from(body.len()).unwrap_or(u32::MAX), &mut out)?;
    out.extend_from_slice(&body);
    Ok(out)
}

fn encode_connect(c: &Connect, out: &mut BytesMut) {
    // §3.1.2 variable header: protocol name "MQTT", level 4, flags, keep-alive.
    encode_str("MQTT", out);
    out.put_u8(4); // protocol level = MQTT 3.1.1
    let flags = if c.clean_session { 0x02 } else { 0x00 };
    out.put_u8(flags);
    out.put_u16(c.keep_alive_secs);
    encode_str(&c.client_id, out);
}

fn encode_connack(c: &ConnAck, out: &mut BytesMut) {
    out.put_u8(if c.session_present { 0x01 } else { 0x00 });
    out.put_u8(c.return_code as u8);
}

fn encode_publish(p: &Publish, out: &mut BytesMut) -> Result<u8, CodecError> {
    encode_str(&p.topic, out);
    if p.qos != QoS::AtMostOnce {
        let pid = p.packet_id.ok_or(CodecError::MissingPacketId)?;
        out.put_u16(pid);
    }
    out.extend_from_slice(&p.payload);
    let flags = ((p.dup as u8) << 3) | ((p.qos as u8) << 1) | (p.retain as u8);
    Ok(flags)
}

pub fn decode_packet(input: &[u8]) -> Result<(Packet, usize), CodecError> {
    if input.is_empty() {
        return Err(CodecError::Underflow { needed: 2 });
    }
    let header = input[0];
    let packet_type = PacketType::from_u8(header >> 4)
        .ok_or(CodecError::UnknownPacketType(header >> 4))?;
    let flags = header & 0x0f;

    let (remaining, len_bytes) = decode_var_int(&input[1..])?;
    let total = 1 + len_bytes + remaining as usize;
    if input.len() < total {
        return Err(CodecError::Underflow { needed: total - input.len() });
    }
    let mut body = &input[1 + len_bytes..total];

    let packet = match packet_type {
        PacketType::Connect => Packet::Connect(decode_connect(&mut body)?),
        PacketType::ConnAck => Packet::ConnAck(decode_connack(&mut body)?),
        PacketType::Publish => Packet::Publish(decode_publish(&mut body, flags)?),
        PacketType::Subscribe => {
            Packet::Subscribe(decode_subscribe(&mut body, flags)?)
        }
        PacketType::SubAck => Packet::SubAck(decode_suback(&mut body)?),
        PacketType::Unsubscribe => {
            Packet::Unsubscribe(decode_unsubscribe(&mut body, flags)?)
        }
        PacketType::UnsubAck => Packet::UnsubAck(decode_unsuback(&mut body)?),
        PacketType::PingReq => {
            decode_empty(body)?;
            Packet::PingReq
        }
        PacketType::PingResp => {
            decode_empty(body)?;
            Packet::PingResp
        }
        PacketType::Disconnect => {
            decode_empty(body)?;
            Packet::Disconnect
        }
        PacketType::PubAck => {
            Packet::PubAck(PubAck { packet_id: decode_packet_id(&mut body)? })
        }
        PacketType::PubRec => {
            Packet::PubRec(PubRec { packet_id: decode_packet_id(&mut body)? })
        }
        PacketType::PubRel => {
            // §3.6.1: PUBREL reserved fixed-header flags MUST be 0b0010.
            if flags != 0x02 {
                return Err(CodecError::BadReservedFlags(flags));
            }
            Packet::PubRel(PubRel { packet_id: decode_packet_id(&mut body)? })
        }
        PacketType::PubComp => Packet::PubComp(PubComp {
            packet_id: decode_packet_id(&mut body)?,
        }),
    };
    Ok((packet, total))
}

fn decode_connect(buf: &mut &[u8]) -> Result<Connect, CodecError> {
    let name = decode_str(buf)?;
    if name != "MQTT" {
        return Err(CodecError::BadProtocolName(name));
    }
    if buf.len() < 4 {
        return Err(CodecError::Underflow { needed: 4 - buf.len() });
    }
    let level = buf[0];
    if level != 4 {
        return Err(CodecError::BadProtocolLevel(level));
    }
    let flags = buf[1];
    let keep_alive = u16::from_be_bytes([buf[2], buf[3]]);
    buf.advance(4);
    let client_id = decode_str(buf)?;
    Ok(Connect {
        client_id,
        clean_session: flags & 0x02 != 0,
        keep_alive_secs: keep_alive,
    })
}

fn decode_connack(buf: &mut &[u8]) -> Result<ConnAck, CodecError> {
    if buf.len() < 2 {
        return Err(CodecError::Underflow { needed: 2 - buf.len() });
    }
    let session_present = buf[0] & 0x01 != 0;
    let code_byte = buf[1];
    buf.advance(2);
    let return_code = ConnAckReturnCode::from_u8(code_byte)
        .ok_or(CodecError::BadConnAckCode(code_byte))?;
    Ok(ConnAck { session_present, return_code })
}

fn decode_publish(buf: &mut &[u8], flags: u8) -> Result<Publish, CodecError> {
    let dup = flags & 0x08 != 0;
    let qos = QoS::from_u8((flags >> 1) & 0x03).ok_or(CodecError::BadQoS((flags >> 1) & 0x03))?;
    let retain = flags & 0x01 != 0;
    let topic = decode_str(buf)?;
    let packet_id = if qos == QoS::AtMostOnce {
        None
    } else {
        if buf.len() < 2 {
            return Err(CodecError::Underflow { needed: 2 - buf.len() });
        }
        let pid = u16::from_be_bytes([buf[0], buf[1]]);
        buf.advance(2);
        Some(pid)
    };
    let payload = Bytes::copy_from_slice(buf);
    Ok(Publish { topic, qos, retain, dup, packet_id, payload })
}

/// MQTT 3.1.1 §3.8 SUBSCRIBE — variable header (packet id) + payload of
/// topic-filter/QoS pairs. Returns the reserved fixed-header flags (0b0010).
fn encode_subscribe(s: &Subscribe, out: &mut BytesMut) -> u8 {
    out.put_u16(s.packet_id);
    for sub in &s.subscriptions {
        encode_str(&sub.topic_filter, out);
        out.put_u8(sub.qos as u8);
    }
    0x02
}

fn decode_subscribe(buf: &mut &[u8], flags: u8) -> Result<Subscribe, CodecError> {
    // §3.8.1: bits 3-0 of byte 1 are reserved and MUST be 0b0010.
    if flags != 0x02 {
        return Err(CodecError::BadReservedFlags(flags));
    }
    if buf.len() < 2 {
        return Err(CodecError::Underflow { needed: 2 - buf.len() });
    }
    let packet_id = u16::from_be_bytes([buf[0], buf[1]]);
    buf.advance(2);
    let mut subscriptions = Vec::new();
    while !buf.is_empty() {
        let topic_filter = decode_str(buf)?;
        if buf.is_empty() {
            return Err(CodecError::Underflow { needed: 1 });
        }
        let qos = QoS::from_u8(buf[0]).ok_or(CodecError::BadQoS(buf[0]))?;
        buf.advance(1);
        subscriptions.push(Subscription { topic_filter, qos });
    }
    // §3.8.3: a SUBSCRIBE with no topic filters is a protocol violation.
    if subscriptions.is_empty() {
        return Err(CodecError::EmptySubscription);
    }
    Ok(Subscribe { packet_id, subscriptions })
}

/// MQTT 3.1.1 §3.9 SUBACK — packet id + one return code per filter.
fn encode_suback(a: &SubAck, out: &mut BytesMut) {
    out.put_u16(a.packet_id);
    for code in &a.return_codes {
        out.put_u8(*code as u8);
    }
}

fn decode_suback(buf: &mut &[u8]) -> Result<SubAck, CodecError> {
    if buf.len() < 2 {
        return Err(CodecError::Underflow { needed: 2 - buf.len() });
    }
    let packet_id = u16::from_be_bytes([buf[0], buf[1]]);
    buf.advance(2);
    let mut return_codes = Vec::with_capacity(buf.len());
    while !buf.is_empty() {
        let code = SubAckReturnCode::from_u8(buf[0])
            .ok_or(CodecError::BadSubAckCode(buf[0]))?;
        buf.advance(1);
        return_codes.push(code);
    }
    Ok(SubAck { packet_id, return_codes })
}

/// Read a 2-byte packet identifier (§2.3.1) — the entire variable header
/// of PUBACK / PUBREC / PUBREL / PUBCOMP (§3.4-§3.7).
fn decode_packet_id(buf: &mut &[u8]) -> Result<u16, CodecError> {
    if buf.len() < 2 {
        return Err(CodecError::Underflow { needed: 2 - buf.len() });
    }
    let id = u16::from_be_bytes([buf[0], buf[1]]);
    buf.advance(2);
    Ok(id)
}

/// Validate that a packet body is empty (§3.12-§3.14: PINGREQ, PINGRESP
/// and DISCONNECT carry no variable header or payload).
fn decode_empty(buf: &[u8]) -> Result<(), CodecError> {
    if buf.is_empty() {
        Ok(())
    } else {
        Err(CodecError::UnexpectedPayload)
    }
}

/// MQTT 3.1.1 §3.10 UNSUBSCRIBE — packet id + a payload of topic filters
/// (no per-filter QoS). Returns the reserved fixed-header flags (0b0010).
fn encode_unsubscribe(u: &Unsubscribe, out: &mut BytesMut) -> u8 {
    out.put_u16(u.packet_id);
    for filter in &u.topic_filters {
        encode_str(filter, out);
    }
    0x02
}

fn decode_unsubscribe(
    buf: &mut &[u8],
    flags: u8,
) -> Result<Unsubscribe, CodecError> {
    // §3.10.1: bits 3-0 of byte 1 are reserved and MUST be 0b0010.
    if flags != 0x02 {
        return Err(CodecError::BadReservedFlags(flags));
    }
    if buf.len() < 2 {
        return Err(CodecError::Underflow { needed: 2 - buf.len() });
    }
    let packet_id = u16::from_be_bytes([buf[0], buf[1]]);
    buf.advance(2);
    let mut topic_filters = Vec::new();
    while !buf.is_empty() {
        topic_filters.push(decode_str(buf)?);
    }
    // §3.10.3: an UNSUBSCRIBE with no topic filters is a protocol violation.
    if topic_filters.is_empty() {
        return Err(CodecError::EmptySubscription);
    }
    Ok(Unsubscribe { packet_id, topic_filters })
}

/// MQTT 3.1.1 §3.11 UNSUBACK — a packet identifier only.
fn encode_unsuback(a: &UnsubAck, out: &mut BytesMut) {
    out.put_u16(a.packet_id);
}

fn decode_unsuback(buf: &mut &[u8]) -> Result<UnsubAck, CodecError> {
    if buf.len() < 2 {
        return Err(CodecError::Underflow { needed: 2 - buf.len() });
    }
    let packet_id = u16::from_be_bytes([buf[0], buf[1]]);
    buf.advance(2);
    Ok(UnsubAck { packet_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puback_round_trip() {
        // §3.4 PUBACK: QoS 1 acknowledgement — packet id only, flags 0.
        let a = PubAck { packet_id: 0x1234 };
        let bytes = encode_packet(&Packet::PubAck(a)).expect("encode");
        assert_eq!(&bytes[..], [0x40, 0x02, 0x12, 0x34].as_slice());
        let (back, used) = decode_packet(&bytes).expect("decode");
        assert_eq!(used, bytes.len());
        assert_eq!(back, Packet::PubAck(a));
    }

    #[test]
    fn pubrec_round_trip() {
        // §3.5 PUBREC: QoS 2 publish received (part 1), flags 0.
        let p = PubRec { packet_id: 0x1234 };
        let bytes = encode_packet(&Packet::PubRec(p)).expect("encode");
        assert_eq!(&bytes[..], [0x50, 0x02, 0x12, 0x34].as_slice());
        let (back, _) = decode_packet(&bytes).expect("decode");
        assert_eq!(back, Packet::PubRec(p));
    }

    #[test]
    fn pubrel_round_trip() {
        // §3.6 PUBREL: QoS 2 publish release (part 2). §3.6.1 reserves the
        // fixed-header flags as 0b0010.
        let p = PubRel { packet_id: 0x1234 };
        let bytes = encode_packet(&Packet::PubRel(p)).expect("encode");
        assert_eq!(&bytes[..], [0x62, 0x02, 0x12, 0x34].as_slice());
        let (back, _) = decode_packet(&bytes).expect("decode");
        assert_eq!(back, Packet::PubRel(p));
    }

    #[test]
    fn pubrel_rejects_wrong_reserved_flags() {
        // §3.6.1: PUBREL fixed-header flags other than 0b0010 are malformed.
        let frame = [0x60, 0x02, 0x12, 0x34];
        assert!(matches!(
            decode_packet(&frame),
            Err(CodecError::BadReservedFlags(0))
        ));
    }

    #[test]
    fn pubcomp_round_trip() {
        // §3.7 PUBCOMP: QoS 2 publish complete (part 3), flags 0.
        let p = PubComp { packet_id: 0x1234 };
        let bytes = encode_packet(&Packet::PubComp(p)).expect("encode");
        assert_eq!(&bytes[..], [0x70, 0x02, 0x12, 0x34].as_slice());
        let (back, _) = decode_packet(&bytes).expect("decode");
        assert_eq!(back, Packet::PubComp(p));
    }

    #[test]
    fn pingreq_round_trip() {
        // §3.12: PINGREQ is a 2-byte packet with zero remaining length.
        let bytes = encode_packet(&Packet::PingReq).expect("encode");
        assert_eq!(&bytes[..], [0xc0, 0x00].as_slice());
        let (back, used) = decode_packet(&bytes).expect("decode");
        assert_eq!(used, 2);
        assert_eq!(back, Packet::PingReq);
    }

    #[test]
    fn pingresp_round_trip() {
        // §3.13: PINGRESP is the server's reply to a PINGREQ.
        let bytes = encode_packet(&Packet::PingResp).expect("encode");
        assert_eq!(&bytes[..], [0xd0, 0x00].as_slice());
        let (back, used) = decode_packet(&bytes).expect("decode");
        assert_eq!(used, 2);
        assert_eq!(back, Packet::PingResp);
    }

    #[test]
    fn disconnect_round_trip() {
        // §3.14: DISCONNECT is the client's clean network teardown.
        let bytes = encode_packet(&Packet::Disconnect).expect("encode");
        assert_eq!(&bytes[..], [0xe0, 0x00].as_slice());
        let (back, used) = decode_packet(&bytes).expect("decode");
        assert_eq!(used, 2);
        assert_eq!(back, Packet::Disconnect);
    }

    #[test]
    fn pingreq_rejects_nonzero_payload() {
        // §3.12: a PINGREQ with a non-empty remaining length is malformed.
        let frame = [0xc0, 0x01, 0x00];
        assert!(matches!(
            decode_packet(&frame),
            Err(CodecError::UnexpectedPayload)
        ));
    }

    #[test]
    fn unsubscribe_round_trip() {
        // §3.10 UNSUBSCRIBE: packet id 11, filters "a/b" and "c/d"
        // (topic filters only — no per-filter QoS byte).
        let u = Unsubscribe {
            packet_id: 11,
            topic_filters: vec!["a/b".into(), "c/d".into()],
        };
        let bytes = encode_packet(&Packet::Unsubscribe(u.clone())).expect("encode");
        // §3.10.1: UNSUBSCRIBE reserved fixed-header flags are 0b0010.
        assert_eq!(bytes[0], 0xa2);
        let (back, used) = decode_packet(&bytes).expect("decode");
        assert_eq!(used, bytes.len());
        assert_eq!(back, Packet::Unsubscribe(u));
    }

    #[test]
    fn unsubscribe_rejects_wrong_reserved_flags() {
        // §3.10.1: byte-1 flags must be 0b0010; here they are 0b0000.
        let frame = [0xa0, 0x05, 0x00, 0x0b, 0x00, 0x01, b'x'];
        assert!(matches!(
            decode_packet(&frame),
            Err(CodecError::BadReservedFlags(0))
        ));
    }

    #[test]
    fn unsubscribe_rejects_empty_filter_list() {
        // §3.10.3: UNSUBSCRIBE MUST carry at least one topic filter.
        let frame = [0xa2, 0x02, 0x00, 0x0b];
        assert!(matches!(
            decode_packet(&frame),
            Err(CodecError::EmptySubscription)
        ));
    }

    #[test]
    fn unsuback_round_trip() {
        // §3.11: UNSUBACK is a packet identifier only; remaining len 2.
        let a = UnsubAck { packet_id: 11 };
        let bytes = encode_packet(&Packet::UnsubAck(a.clone())).expect("encode");
        assert_eq!(&bytes[..], [0xb0, 0x02, 0x00, 0x0b].as_slice());
        let (back, _) = decode_packet(&bytes).expect("decode");
        assert_eq!(back, Packet::UnsubAck(a));
    }

    #[test]
    fn subscribe_round_trip() {
        // §3.8 SUBSCRIBE: packet id 10, "a/b" QoS 1, "c/d" QoS 2.
        let s = Subscribe {
            packet_id: 10,
            subscriptions: vec![
                Subscription { topic_filter: "a/b".into(), qos: QoS::AtLeastOnce },
                Subscription { topic_filter: "c/d".into(), qos: QoS::ExactlyOnce },
            ],
        };
        let bytes = encode_packet(&Packet::Subscribe(s.clone())).expect("encode");
        // §3.8.1: SUBSCRIBE reserved fixed-header flags are 0b0010.
        assert_eq!(bytes[0], 0x82);
        let (back, used) = decode_packet(&bytes).expect("decode");
        assert_eq!(used, bytes.len());
        assert_eq!(back, Packet::Subscribe(s));
    }

    #[test]
    fn subscribe_rejects_wrong_reserved_flags() {
        // §3.8.1: byte-1 flags must be 0b0010; here they are 0b0000.
        let frame = [0x80, 0x06, 0x00, 0x01, 0x00, 0x01, b'x', 0x00];
        assert!(matches!(
            decode_packet(&frame),
            Err(CodecError::BadReservedFlags(0))
        ));
    }

    #[test]
    fn subscribe_rejects_empty_filter_list() {
        // §3.8.3: a SUBSCRIBE MUST carry at least one topic filter.
        let frame = [0x82, 0x02, 0x00, 0x0a];
        assert!(matches!(
            decode_packet(&frame),
            Err(CodecError::EmptySubscription)
        ));
    }

    #[test]
    fn subscribe_rejects_reserved_requested_qos() {
        // §3.8.3: a requested QoS of 3 is a protocol violation.
        let frame = [0x82, 0x06, 0x00, 0x0a, 0x00, 0x01, b'x', 0x03];
        assert!(matches!(decode_packet(&frame), Err(CodecError::BadQoS(3))));
    }

    #[test]
    fn suback_round_trip() {
        // §3.9.3: granted QoS 1, then a failure return code (0x80).
        let a = SubAck {
            packet_id: 10,
            return_codes: vec![
                SubAckReturnCode::MaxQoS1,
                SubAckReturnCode::Failure,
            ],
        };
        let bytes = encode_packet(&Packet::SubAck(a.clone())).expect("encode");
        assert_eq!(&bytes[..], [0x90, 0x04, 0x00, 0x0a, 0x01, 0x80].as_slice());
        let (back, _) = decode_packet(&bytes).expect("decode");
        assert_eq!(back, Packet::SubAck(a));
    }

    #[test]
    fn var_int_round_trip_at_spec_boundaries() {
        // MQTT 3.1.1 §2.2.3 examples: 0, 127, 128, 16383, 16384, 2_097_151, 2_097_152.
        for v in [0u32, 127, 128, 16_383, 16_384, 2_097_151, 2_097_152, 268_435_455] {
            let mut buf = BytesMut::new();
            encode_var_int(v, &mut buf).expect("encode");
            let (decoded, _) = decode_var_int(&buf).expect("decode");
            assert_eq!(decoded, v, "round-trip {v}");
        }
    }

    #[test]
    fn connect_round_trip() {
        let c = Connect {
            client_id: "kitchen-pi".into(),
            clean_session: true,
            keep_alive_secs: 60,
        };
        let bytes = encode_packet(&Packet::Connect(c.clone())).expect("encode");
        let (back, used) = decode_packet(&bytes).expect("decode");
        assert_eq!(used, bytes.len());
        assert_eq!(back, Packet::Connect(c));
    }

    #[test]
    fn connack_round_trip() {
        let c = ConnAck { session_present: false, return_code: ConnAckReturnCode::Accepted };
        let bytes = encode_packet(&Packet::ConnAck(c)).expect("encode");
        let (back, _) = decode_packet(&bytes).expect("decode");
        assert_eq!(back, Packet::ConnAck(c));
    }

    #[test]
    fn publish_qos0_and_qos1_round_trip() {
        let q0 = Publish {
            topic: "home/light/kitchen".into(),
            qos: QoS::AtMostOnce,
            retain: true,
            dup: false,
            packet_id: None,
            payload: Bytes::from_static(b"ON"),
        };
        let bytes = encode_packet(&Packet::Publish(q0.clone())).expect("encode q0");
        let (back, _) = decode_packet(&bytes).expect("decode q0");
        assert_eq!(back, Packet::Publish(q0));

        let q1 = Publish {
            topic: "home/sensor/temp".into(),
            qos: QoS::AtLeastOnce,
            retain: false,
            dup: false,
            packet_id: Some(0x4242),
            payload: Bytes::from_static(b"21.5"),
        };
        let bytes = encode_packet(&Packet::Publish(q1.clone())).expect("encode q1");
        let (back, _) = decode_packet(&bytes).expect("decode q1");
        assert_eq!(back, Packet::Publish(q1));
    }

    #[test]
    fn decode_rejects_non_mqtt_3_1_1_protocol_level() {
        // CONNECT with protocol name MQTT, level 5 (not yet supported here).
        let mut body = BytesMut::new();
        encode_str("MQTT", &mut body);
        body.put_u8(5);
        body.put_u8(0);
        body.put_u16(60);
        encode_str("c", &mut body);
        let mut frame = BytesMut::new();
        frame.put_u8((PacketType::Connect as u8) << 4);
        encode_var_int(body.len() as u32, &mut frame).unwrap();
        frame.extend_from_slice(&body);
        assert!(matches!(decode_packet(&frame), Err(CodecError::BadProtocolLevel(5))));
    }
}
