//! `cave-home-audio-mass` — the music-library & playback-queue brain for
//! cave-home's multi-room audio (ADR-020).
//!
//! This crate is the **domain logic** that turns "play my Morning playlist in
//! the kitchen" into an ordered, navigable playback queue with shuffle, repeat,
//! per-room players, multi-room grouping, and a grandma-friendly status
//! sentence — all as pure, std-only logic with **no** network, no audio device,
//! and no dependency on any other cave-home crate.
//!
//! It is a Music-Assistant-*class* engine, implemented first-party rather than
//! copied: the media model and queue semantics are designed from the music-
//! player problem domain.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`media`] — the [`Track`] / [`Album`] / [`Playlist`] / [`MediaItem`] model
//!   and the [`ProviderId`] source abstraction (typed, never network-bound).
//! - [`shuffle`] — a deterministic, seeded shuffle over a std-only LCG (no
//!   `rand` crate).
//! - [`queue`] — the [`Queue`] engine: enqueue / play-next / play-now / clear /
//!   move / remove, the cursor, [`RepeatMode`], and next/previous computed from
//!   repeat + shuffle together. This is the core.
//! - [`player`] — the per-room [`Player`] (play/pause/idle, elapsed, [`Volume`],
//!   crossfade/gapless prefs, TTS-announcement save/restore) and the multi-room
//!   [`PlayerGroup`] fan-out.
//! - [`library`] — an in-memory [`Library`] with substring search/filter.
//! - [`label`] — the EN / DE / TR status sentences (Charter §6.3).
//!
//! ## What is deferred (Phase 1b, ADR-020)
//!
//! The **music-provider adapters** (local file scan, Spotify / Tidal / etc.),
//! the **streaming / playback pipeline and the Snapcast hand-off**, **metadata /
//! artwork fetch**, and **cave-home-core integration** are network / pipeline
//! bound and deferred — each is enumerated in `parity.manifest.toml`
//! `[[unmapped]]` with an ADR-020 disposition. The actual cross-room *sync* is
//! Snapcast's job (`cave-home-audio-snapcast`); this engine produces the
//! resolved per-room queue and stays out of the real-time path.
//!
//! # Example
//!
//! ```
//! use cave_home_audio_mass::{Lang, MediaItem, Player, Playlist, RepeatMode};
//!
//! // A morning routine queues a playlist on the kitchen player.
//! let morning = Playlist::new("Morning")
//!     .with_track("t1")
//!     .with_track("t2")
//!     .with_track("t3");
//!
//! let mut kitchen = Player::new("Kitchen");
//! kitchen.queue_mut().play_now(MediaItem::Playlist(morning).track_ids());
//! kitchen.queue_mut().set_repeat(RepeatMode::All);
//! kitchen.play();
//!
//! // The household reads a plain sentence, never a track id.
//! assert_eq!(Lang::En.playing_playlist("Morning"), "Playing your Morning playlist");
//!
//! // Skipping forward respects repeat-all: the last song loops to the first.
//! kitchen.queue_mut().select(2);
//! kitchen.next_track();
//! assert_eq!(kitchen.current_track().map(|t| t.as_str()), Some("t1"));
//! ```

pub mod label;
pub mod library;
pub mod media;
pub mod player;
pub mod queue;
pub mod shuffle;

pub use label::Lang;
pub use library::{Library, SearchFields};
pub use media::{Album, Artist, MediaItem, Playlist, ProviderId, Track, TrackId};
pub use player::{PlayState, Player, PlayerGroup, TransitionPrefs, Volume, MAX_VOLUME};
pub use queue::{Queue, RepeatMode};
pub use shuffle::shuffled_order;
