// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `cave-home-esphome` — the `ESPHome` **native-API** protocol codec (ADR-032).
//!
//! `ESPHome` is the most popular way to put a custom ESP32/ESP8266 sensor or
//! switch on a home network. cave-home talks to those devices the same way Home
//! Assistant does: over the `ESPHome` **native API**, a small length-prefixed
//! protobuf protocol on TCP port 6053. This crate is the **pure-logic wire
//! engine** for that protocol — the part that turns socket bytes into framed
//! messages and back.
//!
//! Everything here is **std-only and dependency-free** — no external crates, no
//! network, no TLS, no Noise crypto. It is a *behavioural reimplementation* of
//! the public native-API wire format (the plaintext frame layout and the
//! `api.proto` message-ID table) cross-checked against the MIT `aioesphomeapi`
//! client. The GPL-3.0 `ESPHome` firmware/codegen was **not** read; cave-home is
//! the hub, it only needs to *speak* the protocol (see `parity.manifest.toml`).
//!
//! # Scope (Phase-1 MVP)
//!
//! - [`varint`] — protobuf base-128 (LEB128) unsigned varint, the integer
//!   encoding every native-API field uses.
//! - [`frame`] — the plaintext frame codec: encode + streaming-aware decode of
//!   `<0x00> <varint len> <varint type> <payload>`.
//! - [`message`] — the message-type registry (`api.proto` ids `1..=29`).
//! - [`hash`] — `ESPHome`'s FNV-1 entity-key hash.
//! - [`entity`] — the entity data model ([`EntityKind`], [`EntityCategory`],
//!   [`EntityInfo`]).
//! - [`label`] — grandma-friendly EN/DE/TR descriptions.
//!
//! Everything network/crypto/codegen-bound — the TCP transport, the Noise
//! (encrypted) frame helper, the protobuf message bodies, discovery and core
//! integration — is enumerated in `parity.manifest.toml` as ADR-032 Phase-1b /
//! Phase-2, and the `ESPHome` *firmware* side is a permanent scope-cut.
//!
//! # Example
//!
//! ```
//! use cave_home_esphome::frame::{ApiFrame, FrameDecode};
//!
//! // A PingRequest (message type 7) with an empty body, framed for the wire.
//! let bytes = ApiFrame::new(7, Vec::new()).encode();
//! assert_eq!(bytes, [0x00, 0x00, 0x07]); // preamble, length 0, type 7
//!
//! // It decodes back, reporting how many bytes it consumed.
//! let FrameDecode::Frame { frame, consumed } = ApiFrame::decode(&bytes)? else {
//!     panic!("a whole frame is present");
//! };
//! assert_eq!(frame.message_type, 7);
//! assert_eq!(consumed, bytes.len());
//! # Ok::<(), cave_home_esphome::EsphomeError>(())
//! ```

// A native-API payload is at most a few hundred bytes; its length and the
// message-type id always fit a u32. The `len() as u32` casts are intentional
// and cannot truncate for any real frame.
#![allow(clippy::cast_possible_truncation)]

pub mod entity;
pub mod error;
pub mod frame;
pub mod hash;
pub mod label;
pub mod message;
pub mod varint;

pub use entity::{EntityCategory, EntityInfo, EntityKind};
pub use error::{EsphomeError, Result};
pub use frame::{ApiFrame, FrameDecode};
pub use hash::fnv1_hash;
pub use label::Lang;
pub use message::MessageType;
