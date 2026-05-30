//! MQTT 3.1.1 zero-length control packets — PINGREQ (§3.12), PINGRESP
//! (§3.13), DISCONNECT (§3.14).
//!
//! Clean-room from the OASIS MQTT 3.1.1 specification; no Eclipse
//! Mosquitto source was consulted. Each of these three packets has an
//! empty variable header and payload, so the entire wire form is the
//! two-byte fixed header: `<type << 4> 0x00`.

use cave_home_mqtt::{decode_packet, encode_packet, Packet};

#[test]
fn pingreq_encodes_to_canonical_two_bytes() {
    // §3.12.1: control packet type 12, flags 0, remaining length 0.
    let bytes = encode_packet(&Packet::PingReq).expect("encode PINGREQ");
    assert_eq!(&bytes[..], &[0xC0, 0x00]);
}

#[test]
fn pingresp_encodes_to_canonical_two_bytes() {
    // §3.13.1: control packet type 13, flags 0, remaining length 0.
    let bytes = encode_packet(&Packet::PingResp).expect("encode PINGRESP");
    assert_eq!(&bytes[..], &[0xD0, 0x00]);
}

#[test]
fn disconnect_encodes_to_canonical_two_bytes() {
    // §3.14.1: control packet type 14, flags 0, remaining length 0.
    let bytes = encode_packet(&Packet::Disconnect).expect("encode DISCONNECT");
    assert_eq!(&bytes[..], &[0xE0, 0x00]);
}

#[test]
fn control_packets_round_trip() {
    for p in [Packet::PingReq, Packet::PingResp, Packet::Disconnect] {
        let bytes = encode_packet(&p).expect("encode");
        let (back, used) = decode_packet(&bytes).expect("decode");
        assert_eq!(used, 2, "two-byte fixed header, zero remaining length");
        assert_eq!(back, p);
    }
}

#[test]
fn decode_pingreq_from_wire_bytes() {
    let (pkt, used) = decode_packet(&[0xC0, 0x00]).expect("decode PINGREQ");
    assert_eq!(pkt, Packet::PingReq);
    assert_eq!(used, 2);
}

#[test]
fn control_packet_type_round_trips_via_packet_type() {
    // §2.2.1 — the upper nibble of the fixed header identifies the type.
    use cave_home_mqtt::PacketType;
    assert_eq!(Packet::PingReq.packet_type(), PacketType::PingReq);
    assert_eq!(Packet::PingResp.packet_type(), PacketType::PingResp);
    assert_eq!(Packet::Disconnect.packet_type(), PacketType::Disconnect);
}
