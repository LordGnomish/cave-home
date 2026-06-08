//! Grandma-friendly music status sentences (Charter §6.3, ADR-020).
//!
//! The household never sees "player UUID", "provider URI", "Snapcast" or a
//! playback enum — they read "Playing your Morning playlist", "Next: Reverie by
//! Debussy", or "Music paused", localised to EN / DE / TR (the Charter §6.3
//! languages mandatory from M1). This module is the only place playback state
//! becomes words a person reads.

use crate::media::Track;
use crate::player::{PlayState, Player};

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl PlayState {
    /// A plain-language status line for a player with nothing more specific to
    /// say (no track loaded).
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Playing, Lang::En) => "Music is playing",
            (Self::Playing, Lang::De) => "Musik läuft",
            (Self::Playing, Lang::Tr) => "Müzik çalıyor",
            (Self::Paused, Lang::En) => "Music paused",
            (Self::Paused, Lang::De) => "Musik pausiert",
            (Self::Paused, Lang::Tr) => "Müzik duraklatıldı",
            (Self::Idle, Lang::En) => "Nothing is playing",
            (Self::Idle, Lang::De) => "Es läuft nichts",
            (Self::Idle, Lang::Tr) => "Hiçbir şey çalmıyor",
        }
    }
}

impl Lang {
    /// "Playing X" framing — `{song} by {artist}`, in this language.
    #[must_use]
    fn now_playing(self, track: &Track) -> String {
        match self {
            Self::En => format!("Playing {} by {}", track.title, track.artist),
            Self::De => format!("Spielt {} von {}", track.title, track.artist),
            Self::Tr => format!("{} - {} çalıyor", track.artist, track.title),
        }
    }

    /// "Paused: X" framing while a specific song is held.
    #[must_use]
    fn paused_on(self, track: &Track) -> String {
        match self {
            Self::En => format!("Music paused — {} by {}", track.title, track.artist),
            Self::De => format!("Musik pausiert — {} von {}", track.title, track.artist),
            Self::Tr => format!("Müzik duraklatıldı — {} - {}", track.artist, track.title),
        }
    }

    /// "Playing your X playlist" framing.
    #[must_use]
    pub fn playing_playlist(self, playlist_name: &str) -> String {
        match self {
            Self::En => format!("Playing your {playlist_name} playlist"),
            Self::De => format!("Deine Playlist {playlist_name} läuft"),
            Self::Tr => format!("{playlist_name} listeniz çalıyor"),
        }
    }

    /// "Next: X by Y" framing for the upcoming song.
    #[must_use]
    pub fn up_next(self, track: &Track) -> String {
        match self {
            Self::En => format!("Next: {} by {}", track.title, track.artist),
            Self::De => format!("Als Nächstes: {} von {}", track.title, track.artist),
            Self::Tr => format!("Sıradaki: {} - {}", track.artist, track.title),
        }
    }
}

impl Player {
    /// A single grandma-friendly status sentence for this player.
    ///
    /// When a specific song is loaded it names it ("Playing Reverie by
    /// Debussy"); otherwise it falls back to the plain state line ("Nothing is
    /// playing"). `resolve` looks a track id up in the library so the sentence
    /// can use real titles — the engine never invents metadata.
    #[must_use]
    pub fn status_sentence<'a, F>(&self, lang: Lang, resolve: F) -> String
    where
        F: Fn(&crate::media::TrackId) -> Option<&'a Track>,
    {
        let track = self.current_track().and_then(resolve);
        match (self.state(), track) {
            (PlayState::Playing, Some(t)) => lang.now_playing(t),
            (PlayState::Paused, Some(t)) => lang.paused_on(t),
            (state, _) => state.label(lang).to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{ProviderId, Track, TrackId};
    use crate::player::Player;

    fn track() -> Track {
        Track::new("t1", "Reverie", "Debussy", "Solo Piano", 280, ProviderId::Local)
    }

    #[test]
    fn playing_names_the_song() {
        let mut p = Player::new("Kitchen");
        p.queue_mut().enqueue([TrackId::new("t1")]);
        p.play();
        let t = track();
        let resolve = |id: &TrackId| if id.as_str() == "t1" { Some(&t) } else { None };
        assert_eq!(p.status_sentence(Lang::En, resolve), "Playing Reverie by Debussy");
        assert_eq!(p.status_sentence(Lang::De, resolve), "Spielt Reverie von Debussy");
        assert_eq!(p.status_sentence(Lang::Tr, resolve), "Debussy - Reverie çalıyor");
    }

    #[test]
    fn idle_player_says_nothing_is_playing() {
        let p = Player::new("Den");
        let resolve = |_: &TrackId| None;
        assert_eq!(p.status_sentence(Lang::En, resolve), "Nothing is playing");
        assert_eq!(p.status_sentence(Lang::Tr, resolve), "Hiçbir şey çalmıyor");
    }

    #[test]
    fn paused_names_the_held_song() {
        let mut p = Player::new("Kitchen");
        p.queue_mut().enqueue([TrackId::new("t1")]);
        p.play();
        p.pause();
        let t = track();
        let resolve = |id: &TrackId| if id.as_str() == "t1" { Some(&t) } else { None };
        assert_eq!(p.status_sentence(Lang::En, resolve), "Music paused — Reverie by Debussy");
    }

    #[test]
    fn playing_playlist_phrase() {
        assert_eq!(Lang::En.playing_playlist("Morning"), "Playing your Morning playlist");
        assert_eq!(Lang::De.playing_playlist("Morning"), "Deine Playlist Morning läuft");
        assert_eq!(Lang::Tr.playing_playlist("Morning"), "Morning listeniz çalıyor");
    }

    #[test]
    fn up_next_phrase() {
        let t = track();
        assert_eq!(Lang::En.up_next(&t), "Next: Reverie by Debussy");
        assert_eq!(Lang::De.up_next(&t), "Als Nächstes: Reverie von Debussy");
        assert_eq!(Lang::Tr.up_next(&t), "Sıradaki: Debussy - Reverie");
    }

    #[test]
    fn every_state_has_three_languages() {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            for s in [PlayState::Playing, PlayState::Paused, PlayState::Idle] {
                assert!(!s.label(lang).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-020: the UI must never surface protocol, vendor or
        // implementation terms — only household words.
        const BANNED: &[&str] = &[
            // jargon tokens the UI must never leak:
            "Snapcast", "Mopidy", "provider URI", "media_player", "MQTT",
            "player UUID", "URI", "entity_id", "TCP", "pod", "kubelet", // jargon
            "Queue", "PlayState",
        ];
        let t = track();
        let mut texts: Vec<String> = Vec::new();
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            for s in [PlayState::Playing, PlayState::Paused, PlayState::Idle] {
                texts.push(s.label(lang).to_string());
            }
            texts.push(lang.now_playing(&t));
            texts.push(lang.paused_on(&t));
            texts.push(lang.playing_playlist("Morning"));
            texts.push(lang.up_next(&t));
        }
        for text in texts {
            for banned in BANNED {
                assert!(
                    !text.contains(banned),
                    "UI string leaks jargon {banned:?}: {text}"
                );
            }
        }
    }
}
