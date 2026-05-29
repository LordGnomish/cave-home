//! The MPD `status` and `currentsong` snapshot builders.
//!
//! `status` is the heartbeat reply an MPD client polls: volume, the four toggle
//! flags, the transport state, the queue length, and (when playing) the current
//! song's position, stable id and elapsed time. This module reads the
//! [`Playback`] and [`Tracklist`] models and emits the exact `key: value` field
//! set, using [`crate::response::Response`].

use crate::playback::{PlayState, Playback};
use crate::response::Response;
use crate::tracklist::{Modes, Tracklist};

fn flag(on: bool) -> u8 {
    u8::from(on)
}

/// Build the MPD `status` reply for the current player + queue state.
///
/// Always present: `volume`, `repeat`, `random`, `single`, `consume`,
/// `playlistlength`, `state`. Present only while a song is loaded and not
/// stopped: `song` (position), `songid` (stable id) and `elapsed`.
#[must_use]
pub fn status(playback: &Playback, tracklist: &Tracklist) -> String {
    let m: Modes = playback.modes();
    let mut r = Response::new();
    r.field("volume", playback.volume())
        .field("repeat", flag(m.repeat))
        .field("random", flag(m.random))
        .field("single", flag(m.single))
        .field("consume", flag(m.consume))
        .field("playlistlength", tracklist.len())
        .field("state", playback.state().as_mpd());

    if playback.state() != PlayState::Stop {
        if let Some(pos) = playback.current() {
            if let Some(song) = tracklist.get(pos) {
                r.field("song", pos)
                    .field("songid", song.id)
                    .field("elapsed", format_seconds(playback.elapsed()));
            }
        }
    }
    r.ok()
}

/// Build the MPD `currentsong` reply.
///
/// Returns a bare `OK` when nothing is loaded; otherwise the song's `file`
/// (URI), its `Title`/`Artist` when known, and its `Pos`/`Id`.
#[must_use]
pub fn current_song(playback: &Playback, tracklist: &Tracklist) -> String {
    let mut r = Response::new();
    if let Some(pos) = playback.current() {
        if let Some(song) = tracklist.get(pos) {
            r.field("file", &song.uri);
            if let Some(title) = &song.title {
                r.field("Title", title);
            }
            if let Some(artist) = &song.artist {
                r.field("Artist", artist);
            }
            r.field("Pos", pos).field("Id", song.id);
        }
    }
    r.ok()
}

/// Build the MPD `playlistinfo` reply: one `file`/`Pos`/`Id` block per song, or
/// just the song at `only` when a position is given. An out-of-range `only`
/// yields a bare `OK` (the caller may instead choose to emit an `ACK`).
#[must_use]
pub fn playlist_info(tracklist: &Tracklist, only: Option<usize>) -> String {
    let mut r = Response::new();
    let mut emit = |pos: usize, song: &crate::tracklist::Song| {
        r.field("file", &song.uri);
        if let Some(title) = &song.title {
            r.field("Title", title);
        }
        if let Some(artist) = &song.artist {
            r.field("Artist", artist);
        }
        r.field("Pos", pos).field("Id", song.id);
    };
    match only {
        Some(pos) => {
            if let Some(song) = tracklist.get(pos) {
                emit(pos, song);
            }
        }
        None => {
            for (pos, song) in tracklist.songs().iter().enumerate() {
                emit(pos, song);
            }
        }
    }
    r.ok()
}

/// Format a seconds value the way MPD reports `elapsed` — three decimals.
fn format_seconds(seconds: f64) -> String {
    format!("{seconds:.3}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> (Playback, Tracklist) {
        let mut t = Tracklist::new();
        t.add("local:a", Some("A".into()), Some("Artist A".into()));
        t.add("local:b", None, None);
        let mut p = Playback::new();
        p.set_volume(60).unwrap();
        p.set_repeat(true);
        p.play_at(0);
        p.set_elapsed(12.0).unwrap();
        (p, t)
    }

    #[test]
    fn stopped_status_has_core_fields_but_no_song() {
        let p = Playback::new();
        let t = Tracklist::new();
        let s = status(&p, &t);
        assert!(s.contains("volume: 50\n"));
        assert!(s.contains("repeat: 0\n"));
        assert!(s.contains("random: 0\n"));
        assert!(s.contains("single: 0\n"));
        assert!(s.contains("consume: 0\n"));
        assert!(s.contains("playlistlength: 0\n"));
        assert!(s.contains("state: stop\n"));
        assert!(!s.contains("song:"));
        assert!(!s.contains("elapsed:"));
        assert!(s.ends_with("OK\n"));
    }

    #[test]
    fn playing_status_carries_song_id_and_elapsed() {
        let (p, t) = loaded();
        let s = status(&p, &t);
        assert!(s.contains("state: play\n"));
        assert!(s.contains("volume: 60\n"));
        assert!(s.contains("repeat: 1\n"));
        assert!(s.contains("playlistlength: 2\n"));
        assert!(s.contains("song: 0\n"));
        assert!(s.contains("songid: 0\n"));
        assert!(s.contains("elapsed: 12.000\n"));
    }

    #[test]
    fn currentsong_reports_uri_metadata_pos_and_id() {
        let (p, t) = loaded();
        let s = current_song(&p, &t);
        assert!(s.contains("file: local:a\n"));
        assert!(s.contains("Title: A\n"));
        assert!(s.contains("Artist: Artist A\n"));
        assert!(s.contains("Pos: 0\n"));
        assert!(s.contains("Id: 0\n"));
    }

    #[test]
    fn currentsong_is_bare_ok_when_nothing_loaded() {
        let p = Playback::new();
        let t = Tracklist::new();
        assert_eq!(current_song(&p, &t), "OK\n");
    }

    #[test]
    fn playlistinfo_lists_every_song_in_order() {
        let (_p, t) = loaded();
        let s = playlist_info(&t, None);
        // Two songs -> two Pos lines.
        assert!(s.contains("Pos: 0\n"));
        assert!(s.contains("Pos: 1\n"));
        assert!(s.contains("file: local:a\n"));
        assert!(s.contains("file: local:b\n"));
    }

    #[test]
    fn playlistinfo_with_position_returns_just_that_song() {
        let (_p, t) = loaded();
        let s = playlist_info(&t, Some(1));
        assert!(s.contains("file: local:b\n"));
        assert!(!s.contains("file: local:a\n"));
    }

    #[test]
    fn playlistinfo_out_of_range_is_bare_ok() {
        let (_p, t) = loaded();
        assert_eq!(playlist_info(&t, Some(9)), "OK\n");
    }
}
