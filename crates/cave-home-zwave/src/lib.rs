// SPDX-License-Identifier: Apache-2.0
//! cave-home-zwave — Z-Wave radio stack.
//!
//! Line-by-line port of `packages/zwave-js`, `packages/cc`, `packages/serial`
//! and `packages/core` from `zwave-js/zwave-js` v15.24.0
//! (SHA `5ffca2b38393f9eab0bffcdbd65b3020cbeda492`).
//!
//! Phase 1a MVP scope (per Charter §3.7 + ADR-007):
//! - [`serial`]   — Z-Wave Serial API frame layer (SOF/ACK/NAK/CAN, checksum,
//!                  message envelope) carried over USB UART (the binary wires
//!                  this to `tokio-serial`).
//! - [`security`] — S0 nonce/encryption framework + S2 CKDF bootstrapping.
//! - [`cc`]       — Phase 1a Command Classes shipped today: Basic,
//!                  Binary Switch, Multilevel Switch, Sensor Multilevel,
//!                  Notification, Configuration, Battery. Wake Up,
//!                  Version, Manufacturer Specific land in Phase 1b.
//! - [`events`]   — sink trait the binary wires to the Automation event bus.
//!
//! Out of Phase 1a (tracked in `parity.manifest.toml` `[[unmapped]]`):
//!   driver lifecycle + message queue, controller-side commands,
//!   inclusion / exclusion / heal flows, runtime node-state, prelude
//!   re-exports, and the three remaining CCs above.
//!
//! Charter v2 (ADR-007 / grandma-friendly UX): nothing in this crate is
//! grandma-facing; the user-visible vocabulary ("Cihaz", "Eşle") lives in the
//! Portal admin module + cavectl. This crate never expects an end-user to
//! see a "Home ID", "Node ID", "Command Class" or "Security Class" string.

#![allow(clippy::module_name_repetitions)]

pub mod cc;
pub mod error;
pub mod events;
pub mod security;
pub mod serial;
