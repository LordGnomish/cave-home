// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `cave-home-esphome` — the ESPHome **native-API** protocol codec (ADR-032).
//!
//! ESPHome is the most popular way to put a custom ESP32/ESP8266 sensor or
//! switch on a home network. cave-home talks to those devices the same way Home
//! Assistant does: over the ESPHome **native API**, a small length-prefixed
//! protobuf protocol on TCP port 6053. This crate is the **pure-logic wire
//! engine** for that protocol — the part that turns socket bytes into framed
//! messages and back.
//!
//! Everything here is **std-only and dependency-free** — no external crates, no
//! network, no TLS, no Noise crypto. It is a *behavioural reimplementation* of
//! the public native-API wire format (the plaintext frame layout and the
//! `api.proto` message-ID table) cross-checked against the MIT `aioesphomeapi`
//! client. The GPL-3.0 ESPHome firmware/codegen was **not** read; cave-home is
//! the hub, it only needs to *speak* the protocol (see `parity.manifest.toml`).
//!
//! # Scope (Phase-1 MVP) — being filled in
//!
//! The first slice is the wire framing: [`varint`] (protobuf base-128 / LEB128)
//! and the plaintext [`frame`] codec.

pub mod error;

pub use error::{EsphomeError, Result};
