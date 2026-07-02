// SPDX-License-Identifier: Apache-2.0
//! WebSocket (`v5.channel.k8s.io`) streaming transport for CRI
//! exec / attach / port-forward.
//!
//! ## Why WebSocket and not SPDY
//!
//! The CRI `Exec`/`Attach`/`PortForward` gRPC calls only *negotiate* a
//! streaming URL (see [`super::streaming`] + [`super::RemoteCriClient::exec`]);
//! the actual byte transfer happens over a second, upgraded connection the
//! client opens to that URL. Historically the kubelet dialed it with SPDY/3.1.
//! Kubernetes 1.30 (the pinned CRI version this crate ports — see
//! `parity.manifest.toml`) ships the WebSocket `v5.channel.k8s.io` sub-protocol
//! as a first-class streaming transport, and it is the direction upstream is
//! moving (SPDY is on the deprecation path).
//!
//! WebSocket is also the only transport buildable in this crate's offline,
//! dependency-frozen toolchain: interop-grade SPDY/3.1 requires zlib header
//! compression seeded with the fixed SPDY dictionary (`deflateSetDictionary`),
//! and no dictionary-capable zlib backend is available offline. RFC 6455, by
//! contrast, needs only SHA-1 + base64 for its handshake, both of which we
//! supply locally in [`handshake`] — so this transport adds **no** crates.
//!
//! The SPDY transport remains documented as deferred legacy work.
//!
//! ## Layers
//! - [`handshake`] — RFC 6455 opening-handshake crypto (SHA-1, base64, accept).
//! - [`frame`]     — RFC 6455 frame codec (masking, control frames, lengths).
//! - [`conn`]      — async [`WsConnection`]: client Upgrade + message I/O.
//! - [`proxy`]     — `v5.channel.k8s.io` channel demux + exec/attach/pf bridge.

pub mod conn;
pub mod frame;
pub mod handshake;
pub mod proxy;
