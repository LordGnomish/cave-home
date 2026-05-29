//! `cave-home-node-discovery` — LAN node/peer discovery for multi-node
//! bootstrap (Charter §5 multi-node cluster, ADR-005 deployment topology).
//!
//! When a homeowner adds a second hub (failover) or an ML/GPU box, the new
//! node has to *find* the existing hub on the LAN and present itself for
//! joining. The industry-standard mechanism for zero-configuration LAN
//! discovery is mDNS / DNS-SD (the same Bonjour-class protocol Home Assistant,
//! printers and Chromecasts use). This crate is the **pure-logic core** of that
//! mechanism: the on-the-wire record model, the peer-tracking cache, the
//! self-announcement builder, the version-compatibility policy and the
//! join-token / pairing state machine.
//!
//! # Scope (Phase 1 MVP — pure logic, std-only, no sockets)
//!
//! Implemented, real and tested here:
//! - [`label`] — DNS length-prefixed label encode/decode (the wire primitive).
//! - [`dns_name`] — domain-name <-> wire round-trip built on labels.
//! - [`record`] — the typed PTR / SRV / TXT / A record model for the
//!   `_cavehome._tcp.local` service, including TXT `key=value` encode/decode
//!   and field validation.
//! - [`peer`] — the [`peer::DiscoveredPeer`] model (node id, hostname,
//!   addresses via [`std::net::IpAddr`], port, role, version, TTL, last-seen).
//! - [`registry`] — the peer cache: add/refresh from a record, expire past TTL,
//!   dedupe by node id, detect address changes — pure over a caller-supplied
//!   clock.
//! - [`announce`] — build *this* node's own [`record::ServiceRecord`] set for
//!   advertisement.
//! - [`compat`] — the version-skew policy (Charter §7 always-latest: peers
//!   must be on a compatible version).
//! - [`join`] — the join-token model + the pairing handshake **state machine**
//!   (the user-facing QR/token path per Charter §6.3 / ADR-005).
//!
//! The **actual network** — the UDP multicast socket on the mDNS group, the
//! live announcement loop, DNS message compression-pointer encoding, the real
//! mutual-TLS/PSK pairing crypto, and the `cave-home-cluster` /
//! `cave-home-core` integration — is network/crypto-bound and deferred to
//! Phase 1b. Every deferral is enumerated in `parity.manifest.toml`
//! `[[unmapped]]` with an ADR-005 disposition. Adapters layer those transports
//! on top of this model without changing it.
//!
//! # Example
//!
//! ```
//! use cave_home_node_discovery::{
//!     announce_self, NodeRole, PeerRegistry, ServiceRecord, Lang,
//! };
//! use std::net::{IpAddr, Ipv4Addr};
//!
//! // This hub advertises itself.
//! let me = announce_self(
//!     "hub-kitchen",
//!     "kitchen.local",
//!     NodeRole::Primary,
//!     "1.4.0",
//!     8123,
//!     IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
//! )
//! .expect("valid announcement");
//!
//! // Another hub on the LAN parses our advertisement and tracks us.
//! let mut registry = PeerRegistry::new();
//! let event = registry.observe(&me, /* now_tick = */ 0);
//! assert!(event.is_new());
//!
//! // The grandma-friendly notification the Portal would show.
//! let peer = registry.get("hub-kitchen").expect("just added");
//! assert_eq!(peer.found_message(Lang::En), "Found another hub.");
//! ```

pub mod announce;
pub mod compat;
pub mod dns_name;
pub mod join;
pub mod label;
pub mod peer;
pub mod record;

pub use announce::announce_self;
pub use compat::{compatibility, version_is_compatible, Compatibility, Version, VersionError};
pub use dns_name::{DnsName, NameError};
pub use join::{JoinToken, Pairing, PairingState, TokenError};
pub use label::{decode_label, encode_label, LabelError};
pub use peer::{DiscoveredPeer, Lang, NodeRole, PeerError};
pub use record::{
    ARecord, PtrRecord, RecordError, ServiceRecord, SrvRecord, TxtRecord, SERVICE_TYPE,
};
pub use registry::{ObserveEvent, PeerRegistry};

pub mod registry;
