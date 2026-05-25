//! cave-home-mqtt — clean-room MQTT broker (Phase 1: 3.1.1 wire codec).
//!
//! The upstream Eclipse Mosquitto broker is EPL-2.0/EDL-1.0 (Apache-2.0
//! incompatible for the unified-binary product); cave-home-mqtt is
//! reimplemented from the OASIS MQTT 3.1.1 and 5.0 specifications
//! without reading Mosquitto source. Phase 1 lands the 3.1.1 fixed
//! header + variable-length integer codec and Connect / ConnAck /
//! Publish encode/decode — the slice needed to accept a CONNECT, ack
//! it, and round-trip a PUBLISH. Subscribe / Unsubscribe / Disconnect
//! / Ping land in Phase 1b alongside the session router.

#![doc(html_root_url = "https://docs.rs/cave-home-mqtt")]

pub mod codec;
pub mod packet;

pub use codec::{CodecError, decode_packet, encode_packet};
pub use packet::{
    ConnAck, ConnAckReturnCode, Connect, Packet, PacketType, Publish, QoS,
};
