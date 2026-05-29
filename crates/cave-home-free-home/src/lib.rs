// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `cave-home-free-home` — Busch-Jaeger free@home domain model + datapoint
//! engine (ADR-011).
//!
//! free@home is Busch-Jaeger's residential building-automation brand. It is a
//! *hybrid* system: a friendly device/room/scene surface exposed by the
//! System Access Point, riding on top of the KNX-IP building bus
//! (cave-home-knx is the sibling carrier crate; this crate does **not** depend
//! on it — see ADR-011 for the boundary).
//!
//! This crate is the **brain**: it models the free@home topology, decodes and
//! encodes the string datapoint values the System Access Point speaks, maps a
//! channel's *function* + a datapoint's *pairing role* to a typed meaning,
//! validates control commands, and projects every channel onto one of a small
//! set of grandma-friendly device kinds (light / blind / climate / switch /
//! scene) so the rest of the hub treats a free@home blind exactly like a
//! Zigbee one.
//!
//! # Scope (Phase 1 MVP — pure logic, std-only, no network)
//!
//! Implemented, real and tested here:
//! - [`id`] — the free@home id scheme: device serial, channel id (`ch0003`),
//!   datapoint id (`odp0000` / `idp0001`) — parse, format, round-trip.
//! - [`function`] — the curated set of documented function IDs (switch,
//!   dimmer, blind, room-temperature controller, scene, …).
//! - [`pairing`] — the datapoint *roles* (switch on/off, set/actual brightness,
//!   set/current temperature, move up/down) + a typed value-shape per role.
//! - [`value`] — the datapoint value codec: typed values
//!   (bool / percent 0..=100 / temperature) ↔ the wire string form, with
//!   bounds + rounding.
//! - [`command`] — building and validating a [`command::SetDatapoint`] against
//!   the pairing's expected value shape.
//! - [`topology`] — the typed [`topology::SysAp`] → [`topology::Device`] →
//!   [`topology::Channel`] → [`topology::Datapoint`] tree, and parsing the
//!   "get-all" devices response into it.
//! - [`mapping`] — projecting a channel's function onto a cave-home
//!   [`mapping::DeviceKind`].
//! - [`label`] — localised, jargon-free EN / DE / TR phrases for the actions a
//!   household actually takes (Charter §6.3, ADR-007).
//!
//! Deferred to Phase 1b (network/transport bound; see `parity.manifest.toml`
//! `[[unmapped]]`): the System Access Point local HTTP REST + WebSocket update
//! transport, authentication, the scenes/timer *programming* API, the KNX-IP
//! bridge tie-in, and cave-home-core integration.
//!
//! # Example
//!
//! ```
//! use cave_home_free_home::{
//!     ChannelId, Function, DeviceKind, SetDatapoint, Pairing, Lang, action_phrase, Action,
//! };
//!
//! // A dimmable living-room light, addressed by serial + channel.
//! let channel = ChannelId::parse("ch0003").unwrap();
//! assert_eq!(channel.index(), 3);
//! assert_eq!(Function::DimmingActuator.device_kind(), DeviceKind::Light);
//!
//! // Turn it to 50 %: validated against the "set brightness" pairing role.
//! let cmd = SetDatapoint::percent("ABB700C12345", channel, Pairing::SetBrightness, 50).unwrap();
//! assert_eq!(cmd.wire_value(), "50");
//!
//! // What grandma sees, in German:
//! let phrase = action_phrase(Lang::De, "Wohnzimmer", Action::Brightness(50));
//! assert_eq!(phrase, "Wohnzimmer auf 50 %");
//! ```

#![forbid(unsafe_code)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]

pub mod command;
pub mod function;
pub mod id;
pub mod label;
pub mod mapping;
pub mod pairing;
pub mod topology;
pub mod value;

pub use command::{CommandError, SetDatapoint};
pub use function::Function;
pub use id::{ChannelId, DatapointId, DeviceSerial, Direction, IdError};
pub use label::{action_phrase, Action, Lang};
pub use mapping::DeviceKind;
pub use pairing::{Pairing, ValueShape};
pub use topology::{Channel, Datapoint, Device, ParseError, SysAp};
pub use value::{Value, ValueError};
