//! `cave-home-display` — TV / display control model & engine (ADR-028).
//!
//! This crate is the **brain** for controlling a television or media player in a
//! cloud-free home: it owns the power model, the playback state machine (the
//! Home Assistant `media_player` domain semantics, permissive), the volume value
//! object with a parental cap and mute-restore, the input-source and installed-
//! app catalog with validation and capability gating, the command state machine
//! that ties them together under power gating, and the grandma-friendly status
//! sentence a household reads — all as pure, std-only logic with no vendor, radio
//! or network dependency.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`power`] — the [`PowerState`] (On / Off / Standby) model.
//! - [`playback`] — the [`PlaybackState`] set and its transition rules.
//! - [`volume`] — the [`Volume`] value object: 0..=100 clamping, a parental cap,
//!   volume-up/down steps, and mute that preserves the level for unmute.
//! - [`source`] — the [`Source`] / [`App`] model and the [`SourceCatalog`] with
//!   known-input and installed-app validation plus app-capability gating.
//! - [`machine`] — the [`Display`] state machine: [`MediaCommand`] application,
//!   power gating, playback transitions, source/app validation, volume clamping.
//! - [`label`] — the EN / DE / TR status sentence (Charter §6.3, ADR-028).
//!
//! The **vendor I/O adapters** (LG webOS, Samsung Tizen, Android TV / Google
//! Cast, Apple TV, Roku, HDMI-CEC, DLNA), **now-playing metadata / artwork
//! fetch**, and **cave-home-core entity/state integration** are network /
//! hardware bound and deferred to Phase 1b — each is enumerated in
//! `parity.manifest.toml` `[[unmapped]]` with an ADR-028 disposition. They map
//! their wire protocols onto this engine's [`MediaCommand`] / [`Display`] model
//! and reuse it unchanged.
//!
//! # Example
//!
//! ```
//! use cave_home_display::{Display, Lang, MediaCommand, SourceCatalog, Volume};
//!
//! // A typical smart TV (HDMI inputs + a couple of apps), off, quiet.
//! let mut tv = Display::new(SourceCatalog::typical_smart_tv(), Volume::new(20));
//!
//! // While it is off, the household reads "The TV is off".
//! assert_eq!(tv.status_sentence(Lang::En), "The TV is off");
//!
//! // Wake it, open an app, and play.
//! tv.apply(MediaCommand::PowerOn)?;
//! tv.apply(MediaCommand::LaunchApp("netflix".into()))?;
//! tv.apply(MediaCommand::Play)?;
//! assert_eq!(tv.status_sentence(Lang::En), "The TV is playing");
//!
//! // A parental cap clamps the volume rather than blasting the room.
//! tv.apply(MediaCommand::SetVolume(95))?;
//! assert!(tv.volume().level() <= 100);
//! # Ok::<(), cave_home_display::CommandError>(())
//! ```

pub mod label;
pub mod machine;
pub mod playback;
pub mod power;
pub mod source;
pub mod volume;

pub use label::Lang;
pub use machine::{CommandError, Display, MediaCommand};
pub use playback::PlaybackState;
pub use power::PowerState;
pub use source::{App, AppCapability, Source, SourceCatalog};
pub use volume::{Volume, MAX_VOLUME};
