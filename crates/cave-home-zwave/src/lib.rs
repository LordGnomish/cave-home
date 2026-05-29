// SPDX-License-Identifier: Apache-2.0
//! `cave-home-zwave` — a Z-Wave Command Class encode/decode + value-model engine.
//!
//! This crate is the **brain** that turns a raw Z-Wave application payload into
//! something the rest of cave-home can reason about, and back again. It models
//! the Z-Wave Command Class (CC) framing, decodes the core Command Classes a
//! home actually uses into typed commands, projects those onto a protocol- and
//! vendor-neutral [`value::Value`], and renders that value as a grandma-friendly
//! sentence in EN / DE / TR.
//!
//! It is **pure logic**: std-only, no external crates, no network, no hardware,
//! no `unsafe`. Every parser returns a [`error::ZwaveResult`] and never panics
//! on malformed input.
//!
//! # Scope (Phase 1 MVP)
//!
//! - [`command_class`] — the [`CommandClass`] id enum + the typed [`Command`]
//!   model, with [`Command::decode`] / [`Command::encode`]. Implemented Command
//!   Classes: Basic, Binary Switch, Multilevel Switch, Binary Sensor,
//!   Multilevel Sensor, Meter, Color Switch, Thermostat Setpoint, Configuration,
//!   Notification, Battery.
//! - [`sensor_decode`] — the shared precision/scale/size fixed-point value
//!   encoding used by Multilevel Sensor, Meter and Thermostat Setpoint.
//! - [`value`] — the typed [`Value`] model the decoders produce.
//! - [`address`] — node + endpoint addressing and a household device-role hint.
//! - [`label`] — grandma-friendly localized rendering (Charter §6.3, ADR-007).
//!
//! Provenance: implemented first-party from the **public** Silicon Labs Z-Wave
//! Command Class specification (clean-room). Per ADR-001, node-zwave-js was the
//! ecosystem reference; no GPL/AGPL source was read or ported.
//!
//! The transport half of Z-Wave — the Serial API / Z/IP link to a 700/800-series
//! controller, inclusion / exclusion + S2 security, network management,
//! association groups and OTA — is hardware/serial/crypto-bound and is deferred
//! to Phase 1b, enumerated in `parity.manifest.toml` under `[[unmapped]]`.
//!
//! # Example
//!
//! ```
//! use cave_home_zwave::{Command, Lang, describe};
//!
//! // A bedroom switch reports it just turned on (Binary Switch Report, 0xFF).
//! let cmd = Command::decode(&[0x25, 0x03, 0xFF]).expect("valid report");
//! let value = cmd.to_value().expect("switch carries a value");
//! assert_eq!(describe(&value, Some("Bedroom"), Lang::En), "Bedroom switch on");
//!
//! // And it round-trips back to the same bytes.
//! assert_eq!(cmd.encode(), vec![0x25, 0x03, 0xFF]);
//! ```

pub mod address;
pub mod command_class;
pub mod error;
pub mod label;
pub mod sensor_decode;
pub mod value;

pub use address::{Address, DeviceRole};
pub use command_class::{Command, CommandClass, LevelChange};
pub use error::{ZwaveError, ZwaveResult};
pub use label::{describe, Lang};
pub use sensor_decode::FixedPoint;
pub use value::{Quantity, TemperatureUnit, Value};
