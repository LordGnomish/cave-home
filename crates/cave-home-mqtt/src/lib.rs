//! cave-home-mqtt — clean-room MQTT 5.0 broker.
//!
//! The upstream Eclipse Mosquitto broker is EPL-2.0/EDL-1.0 (Apache-2.0
//! incompatible for the unified-binary product); cave-home-mqtt is
//! reimplemented from the OASIS MQTT 3.1.1 and 5.0 specifications
//! without reading Mosquitto source.
//!
//! Layers, bottom to top:
//!
//! * [`codec`] / [`packet`] — the complete MQTT 3.1.1 control-packet wire
//!   codec (all 14 packet types).
//! * [`v5`] — the MQTT 5.0 codec: §2.2.2 properties, §2.4 reason codes and
//!   all 15 v5 control packets (CONNECT level 5 … the new AUTH).
//! * [`broker`] — the I/O-free decision core: topic-filter wildcard
//!   matching (§4.7) + shared subscriptions (§4.8.2), retained store
//!   (§3.3.1.3), per-client sessions with QoS 1/2 flow (§4.3),
//!   username/password + topic ACL, Last Will & Testament, and the
//!   [`Broker`](broker::Broker) router that turns packets into
//!   [`Action`](broker::Action)s. Pluggable automation
//!   [`hooks`](broker::hooks).
//! * [`metrics`] — broker counters + Prometheus exposition.
//! * [`bridge`] — Mosquitto-compatible bridge topic mapping.
//! * [`runtime`] (feature `runtime`) — async TCP / TLS / WebSocket
//!   listeners that drive the core over real sockets.

#![doc(html_root_url = "https://docs.rs/cave-home-mqtt")]

pub mod bridge;
pub mod broker;
pub mod codec;
pub mod metrics;
pub mod packet;
#[cfg(feature = "runtime")]
pub mod runtime;
pub mod v5;

pub use codec::{CodecError, decode_packet, encode_packet};
pub use packet::{
    ConnAck, ConnAckReturnCode, Connect, Packet, PacketType, PubAck, PubComp,
    PubRec, PubRel, Publish, QoS, SubAck, SubAckReturnCode, Subscribe,
    Subscription, UnsubAck, Unsubscribe,
};
