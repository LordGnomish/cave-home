//! `cave-home-audio-mopidy` — the local music-server protocol & state engine
//! (ADR-020, ROADMAP M9).
//!
//! cave-home's local music server speaks the **MPD line protocol** (the same
//! protocol Mopidy exposes to MPD clients). This crate is the **protocol and
//! playback-state core** of that server: it parses client request lines into
//! typed commands, models the play queue and transport state, computes the next
//! song under every playback mode, and formats MPD-style replies — plus a
//! grandma-friendly localisation layer for the Portal and voice assistant.
//!
//! It is implemented from the *public, documented* MPD text protocol; no GPL
//! MPD server source was read (Charter §6.1). Mopidy itself is Apache-2.0.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`command`] — parse one MPD request line into a [`Command`] (robust to
//!   quoted/escaped arguments; errors instead of panicking).
//! - [`tracklist`] — the play queue: stable song ids vs. positions, add /
//!   delete / move / clear, and the **next-song** computation honouring
//!   `random` / `repeat` / `single` / `consume`.
//! - [`playback`] — the transport state: play / pause / stop, volume 0..=100,
//!   current position, elapsed, the toggle modes.
//! - [`response`] / [`status`] — format `key: value` replies terminated by `OK`
//!   or `ACK [error]`, and build the `status` / `currentsong` / `playlistinfo`
//!   snapshots.
//! - [`label`] — EN / DE / TR household phrases ("Playing … by …", "Music
//!   stopped", "Repeat is on"); the wire protocol never reaches the user
//!   (Charter §6.3, ADR-007).
//!
//! Deferred to Phase 1b — all network/pipeline-bound, all enumerated in
//! `parity.manifest.toml` `[[unmapped]]` with an ADR-020 disposition: the TCP
//! MPD server (idle / command-list / binary responses), the GStreamer audio
//! playback pipeline, Mopidy backend extensions (local / Spotify / TuneIn / …),
//! library browsing over real backends, and the cave-home-core + Snapcast
//! multi-room integration. They drive this engine; they add no new core logic.
//!
//! # Example
//!
//! ```
//! use cave_home_audio_mopidy::{parse, Command, Playback, Tracklist, Modes, NextSong};
//!
//! // A client asks to queue a song, then play it.
//! let mut queue = Tracklist::new();
//! let mut player = Playback::new();
//!
//! assert_eq!(
//!     parse("add \"local:track:Yesterday.mp3\""),
//!     Ok(Command::Add("local:track:Yesterday.mp3".to_owned()))
//! );
//! let id = queue.add("local:track:Yesterday.mp3", Some("Yesterday".into()), Some("The Beatles".into()));
//! player.play_at(0);
//!
//! // When the song finishes, the engine picks the next one. With a one-song
//! // queue and no repeat, that means "stop".
//! assert_eq!(queue.next_after(0, Modes::default(), 0), NextSong::Stop);
//! assert_eq!(queue.position_of_id(id), Some(0));
//! ```

pub mod command;
pub mod label;
pub mod playback;
pub mod response;
pub mod status;
pub mod tracklist;

pub use command::{parse, Command, ParseError};
pub use label::Lang;
pub use playback::{PlayState, Playback, PlaybackError};
pub use response::{ack, ok, AckError, Response};
pub use status::{current_song, playlist_info, status};
pub use tracklist::{Modes, NextSong, QueueError, Song, Tracklist};
