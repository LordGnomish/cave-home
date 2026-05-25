//! MQTT 3.1.1 wire codec — clean-room from OASIS §2.2 fixed header,
//! §2.2.3 variable-length integer, and §3.1/§3.2/§3.3 packet bodies.

use crate::packet::{
    ConnAck, ConnAckReturnCode, Connect, Packet, PacketType, Publish, QoS,
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
        other => return Err(CodecError::UnsupportedInPhase1(other)),
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

#[cfg(test)]
mod tests {
    use super::*;

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
