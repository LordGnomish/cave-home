//! `cave-home-lighting-wled` — WLED light control brain for cave-home (ADR-014).
//!
//! This crate is the **logic** that turns "make the living-room warm white at
//! 60%" into a validated WLED device state, and turns a device state back into
//! a sentence a household understands. It models the WLED JSON API `state`
//! object (colours, segments, effects, palettes, nightlight), provides a pure
//! validated command layer, and renders everything to grandma-friendly EN / DE
//! / TR (Charter §6.3, ADR-007).
//!
//! # Clean-room (Charter §6.1 / ADR-014)
//!
//! Implemented strictly from the **public WLED JSON API documentation**; WLED
//! firmware source was **not** read or ported. The spec sources are recorded in
//! `parity.manifest.toml`.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`color`] — [`Rgb`] / [`Rgbw`], HSV↔RGB, Kelvin→RGB white approximation.
//! - [`segment`] — the WLED [`Segment`] model (colour slots, effect, palette,
//!   speed/intensity, orientation flags).
//! - [`state`] — the device [`State`] object with [`state::Nightlight`], with
//!   std-only JSON round-trip ([`state::State::to_json`] /
//!   [`state::State::from_json`]).
//! - [`effect`] — curated registries of documented built-in effects/palettes
//!   with friendly localised names.
//! - [`command`] — the pure, validated [`Command`] layer + the
//!   [`command::headline`] one-liner.
//! - [`label`] — the [`Lang`] enum + colour/brightness wording.
//!
//! The real-device **transports** (HTTP/JSON, WebSocket, UDP realtime —
//! DDP/E1.31/DRGB), mDNS discovery, the full 100+ effect/palette enumeration,
//! and cave-home-core integration are network/hardware-bound and deferred to
//! Phase 1b — every one is enumerated in `parity.manifest.toml` `[[unmapped]]`
//! with an ADR-014 disposition.
//!
//! # Example
//!
//! ```
//! use cave_home_lighting_wled::{Command, Lang, Rgb, Segment, State};
//! use cave_home_lighting_wled::command::headline;
//!
//! // A strip with one 30-LED segment.
//! let mut light = State::default();
//! light.segments = vec![Segment::solid(0, 0, 30, Rgb::WHITE).expect("valid range")];
//!
//! // "Make it warm white at 60%."
//! let warm = Rgb::new(255, 220, 180);
//! let light = Command::SetSegmentColor { segment: 0, color: warm }
//!     .apply(&light)
//!     .expect("segment 0 exists");
//! let light = Command::SetBrightness(153).apply(&light).expect("valid");
//!
//! assert_eq!(headline("Living-room", &light, Lang::En),
//!            "Living-room lights are warm white at 60%");
//!
//! // `kelvin_to_rgb` gives the byte colour for a colour temperature.
//! let amber = cave_home_lighting_wled::color::kelvin_to_rgb(2700);
//! assert_eq!(amber.r, 255); // warm temperatures are red-saturated
//!
//! // The state round-trips the documented WLED JSON shape.
//! let json = light.to_json();
//! assert_eq!(State::from_json(&json).expect("decode"), light);
//! ```

pub mod color;
pub mod command;
pub mod effect;
pub mod json;
pub mod label;
pub mod segment;
pub mod state;

pub use color::{kelvin_to_rgb, Hsv, Rgb, Rgbw};
pub use command::{headline, Command, CommandError};
pub use effect::{effect_name, Effect, Palette, EFFECTS, PALETTES};
pub use label::Lang;
pub use segment::Segment;
pub use state::{Nightlight, State};
