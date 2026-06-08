//! `cave-home-audio-snapcast` — the multi-room synchronised-audio control brain
//! (ADR-020, ROADMAP M9).
//!
//! **Clean-room crate (Charter §6.1 / ADR-020).** Snapcast upstream is GPL-3.0;
//! its source was **NOT read or ported**. Everything here is implemented from
//! the *public* Snapcast control-protocol documentation (the JSON-RPC 2.0
//! `Server.GetStatus` tree shape and the documented control verbs). Contributors
//! who have read Snapcast source are recused from this crate.
//!
//! This crate is the **control plane**: it models a house full of synchronised
//! speakers and the pure operations that re-arrange them, with no network and no
//! audio pipeline. It turns "play the kitchen and living room together at a
//! comfortable volume" into validated state changes and a grandma-friendly
//! one-line headline — never exposing client ids, codecs, or JSON-RPC method
//! names to the household.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`topology`] — the `Server.GetStatus` tree ([`Stream`], the speaker/group
//!   sets) and the "every speaker in exactly one group" invariant.
//! - [`client`] — a speaker and its validated [`Volume`] value object.
//! - [`group`] — a synchronised group plus the group-volume arithmetic
//!   (average of unmuted members; proportional, clamped spread).
//! - [`control`] — the pure, validated control operations and the localised
//!   headline (Charter §6.3, ADR-007).
//! - [`sync`] — the time-sync value object ([`LatencyMs`]) and the
//!   per-client play-delay / shared-buffer arithmetic.
//! - [`rpc`] — a typed JSON-RPC 2.0 request / notification model (method names +
//!   param shapes), round-tripped through a std-only JSON helper.
//! - [`json`] — that minimal std-only JSON value model.
//! - [`label`] — the EN / DE / TR localisation helpers.
//!
//! # Deferred to Phase 1b (network / audio-pipeline bound)
//!
//! The actual TCP JSON-RPC transport and live notification stream, the
//! snapserver/snapclient audio pipeline (PCM streaming and the real wire-level
//! time-sync handshake), the source/stream backends (Spotify / `AirPlay` / pipe)
//! and the cave-home-core integration are all enumerated in
//! `parity.manifest.toml` `[[unmapped]]` with an ADR-020 disposition. The
//! control brain here is transport-agnostic and depends on no other cave-home
//! crate.
//!
//! # Example
//!
//! ```
//! use cave_home_audio_snapcast::{
//!     control, topology::{Topology, Stream, StreamStatus},
//!     group::Group, client::{Client, Volume}, label::Lang,
//! };
//!
//! let mut house = Topology::new();
//! house.add_stream(Stream::new("playlist", StreamStatus::Playing, "flac", "48000:16:2"));
//! house.add_group(
//!     Group::new("g1", "Kitchen", "playlist", vec!["k".into()]),
//!     vec![Client::new("k", "Kitchen", Volume::new(40).unwrap())],
//! ).unwrap();
//! house.add_group(
//!     Group::new("g2", "Living room", "playlist", vec!["l".into()]),
//!     vec![Client::new("l", "Living room", Volume::new(80).unwrap())],
//! ).unwrap();
//!
//! // Merge them into one group so they play in sync, then level the group.
//! control::create_group(&mut house, "together", "Downstairs", "playlist", &["k", "l"]).unwrap();
//! control::set_group_volume(&mut house, "together", Volume::new(60).unwrap()).unwrap();
//!
//! assert!(house.invariant_holds());
//! // Household-facing summary — no protocol terms.
//! assert_eq!(control::headline(&house, Lang::En), "Music in every room");
//! ```

pub mod client;
pub mod control;
pub mod group;
pub mod json;
pub mod label;
pub mod rpc;
pub mod sync;
pub mod topology;

pub use client::{Client, Volume, VolumeError};
pub use control::{headline, ControlError};
pub use group::Group;
pub use json::Json;
pub use label::Lang;
pub use rpc::{Notification, Request};
pub use sync::{client_delay_ms, LatencyMs};
pub use topology::{Stream, StreamStatus, Topology, TopologyError};
