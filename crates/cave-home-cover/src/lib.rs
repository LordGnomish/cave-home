//! `cave-home-cover` — covers, garage doors, blinds, awnings (ADR-015).
//!
//! This crate is the **brain** for everything in the home that opens and
//! closes on a motor: garage doors, motorised blinds / shutters, retractable
//! awnings, curtains, gates and powered windows. It models a cover's position
//! and tilt, validates and applies the cover commands, infers travel direction,
//! enforces what a given device can physically do, handles the safety stop /
//! obstruction case, and turns the result into a grandma-friendly sentence in
//! EN / DE / TR. It mirrors the Home Assistant `cover` entity-domain semantics
//! (Apache-2.0), implemented from those public semantics — no GPL source.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`position`] — the validated 0..=100 [`Position`] value object.
//! - [`state`] — the five-value [`CoverState`] lifecycle + position-derived
//!   at-rest state.
//! - [`device_class`] — the nine [`DeviceClass`]es and their
//!   capability/[`Features`] model.
//! - [`machine`] — the [`Cover`] position state machine: command application,
//!   direction inference, independent tilt axis, always-on Stop, and the
//!   obstruction safety override.
//! - [`label`] — grandma-friendly localised status sentences (Charter §6.3,
//!   ADR-007).
//!
//! The **vendor I/O adapters** (OpenGarage, ESPHome cover bindings, Somfy RTS,
//! and the Zigbee / Z-Wave / Matter / MQTT cover paths), real motor
//! travel-time calibration, and the cave-home-core entity integration are all
//! network/RF/hardware-bound and deferred to Phase 1b — every one is enumerated
//! in `parity.manifest.toml` `[[unmapped]]` with an ADR-015 disposition. They
//! drive this same engine; they add transport, not new cover logic.
//!
//! # Example
//!
//! ```
//! use cave_home_cover::{Cover, CoverCommand, CoverState, DeviceClass, Lang, Position};
//!
//! // A motorised venetian blind: positions and tilts.
//! let mut blind = Cover::with_class_defaults(DeviceClass::Blind);
//!
//! // Drive it half open, then tilt the slats — tilt does not move the blind.
//! blind.apply(CoverCommand::SetPosition(Position::new(50).unwrap())).unwrap();
//! blind.apply(CoverCommand::SetTiltPosition(Position::new(20).unwrap())).unwrap();
//! assert_eq!(blind.position(), Position::new(50).unwrap());
//! assert_eq!(blind.state(), CoverState::Stopped);
//!
//! // The household sees plain language, never a percentage.
//! let said = cave_home_cover::status_sentence(
//!     blind.class(), blind.state(), blind.position(), Lang::En,
//! );
//! assert_eq!(said, "The blinds is half open.");
//! ```

pub mod device_class;
pub mod label;
pub mod machine;
pub mod position;
pub mod state;

pub use device_class::{DeviceClass, Features};
pub use label::{status_sentence, Lang};
pub use machine::{CommandError, Cover, CoverCommand};
pub use position::{Position, PositionError};
pub use state::CoverState;
