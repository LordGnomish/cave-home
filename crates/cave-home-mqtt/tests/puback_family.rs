//! MQTT 3.1.1 QoS acknowledgement packets — PUBACK (§3.4), PUBREC
//! (§3.5), PUBREL (§3.6), PUBCOMP (§3.7).
//!
//! Clean-room from the OASIS MQTT 3.1.1 specification; no Eclipse
//! Mosquitto source was consulted. Each of these four packets has a
//! variable header consisting solely of the 2-byte Packet Identifier
//! and no payload, so the wire form is `<type<<4 | flags> 0x02 <id-hi>
//! <id-lo>`. PUBREL (§3.6.1) is the lone member whose fixed-header
//! flags are the reserved value 0010 rather than 0000.

use cave_home_mqtt::{decode_packet, encode_packet, Packet, PacketType};

#[test]
fn puback_encodes_packet_identifier() {
    // §3.4: type 4, flags 0000, remaining length 2, packet id 0x1234.
    let bytes = encode_packet(&Packet::PubAck(0x1234)).expect("encode PUBACK");
    assert_eq!(&bytes[..], &[0x40, 0x02, 0x12, 0x34]);
}

#[test]
fn pubrec_encodes_packet_identifier() {
    // §3.5: type 5, flags 0000.
    let bytes = encode_packet(&Packet::PubRec(0x1234)).expect("encode PUBREC");
    assert_eq!(&bytes[..], &[0x50, 0x02, 0x12, 0x34]);
}

#[test]
fn pubrel_uses_reserved_flags_0010() {
    // §3.6.1: PUBREL fixed-header flags MUST be 0010, so the first byte
    // is 0x62 (type 6 << 4 | 0x2), not 0x60.
    let bytes = encode_packet(&Packet::PubRel(0x1234)).expect("encode PUBREL");
    assert_eq!(&bytes[..], &[0x62, 0x02, 0x12, 0x34]);
}

#[test]
fn pubcomp_encodes_packet_identifier() {
    // §3.7: type 7, flags 0000.
    let bytes = encode_packet(&Packet::PubComp(0x1234)).expect("encode PUBCOMP");
    assert_eq!(&bytes[..], &[0x70, 0x02, 0x12, 0x34]);
}

#[test]
fn ack_packets_round_trip() {
    let cases = [
        Packet::PubAck(1),
        Packet::PubRec(40_000),
        Packet::PubRel(0xFFFF),
        Packet::PubComp(7),
    ];
    for p in cases {
        let bytes = encode_packet(&p).expect("encode");
        let (back, used) = decode_packet(&bytes).expect("decode");
        assert_eq!(used, 4, "1 header + 1 length + 2 packet-id bytes");
        assert_eq!(back, p);
    }
}

#[test]
fn decode_pubrel_from_wire_bytes() {
    let (pkt, used) = decode_packet(&[0x62, 0x02, 0x12, 0x34]).expect("decode PUBREL");
    assert_eq!(pkt, Packet::PubRel(0x1234));
    assert_eq!(used, 4);
}

#[test]
fn ack_packet_types_map_through_packet_type() {
    assert_eq!(Packet::PubAck(1).packet_type(), PacketType::PubAck);
    assert_eq!(Packet::PubRec(1).packet_type(), PacketType::PubRec);
    assert_eq!(Packet::PubRel(1).packet_type(), PacketType::PubRel);
    assert_eq!(Packet::PubComp(1).packet_type(), PacketType::PubComp);
}
