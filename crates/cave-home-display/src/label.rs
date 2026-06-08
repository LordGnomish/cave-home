//! Grandma-friendly status sentences for a display (Charter §6.3, ADR-028).
//!
//! The household never sees "media_player", "CEC logical address", "DLNA" or a
//! playback enum — they see "The TV is playing", "The TV is off", or "The sound
//! is muted", localised to EN / DE / TR (the Charter §6.3 languages mandatory
//! from M1). This module is the only place display state becomes words a person
//! reads.

use crate::machine::Display;
use crate::playback::PlaybackState;
use crate::power::PowerState;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl PowerState {
    /// A plain-language status line for this power state.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::On, Lang::En) => "The TV is on",
            (Self::On, Lang::De) => "Der Fernseher ist an",
            (Self::On, Lang::Tr) => "Televizyon açık",
            // Off and standby both read as "off" to a person in the room.
            (Self::Off | Self::Standby, Lang::En) => "The TV is off",
            (Self::Off | Self::Standby, Lang::De) => "Der Fernseher ist aus",
            (Self::Off | Self::Standby, Lang::Tr) => "Televizyon kapalı",
        }
    }
}

impl PlaybackState {
    /// A plain-language status line for what the TV is doing with its content.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Playing, Lang::En) => "The TV is playing",
            (Self::Playing, Lang::De) => "Der Fernseher spielt",
            (Self::Playing, Lang::Tr) => "Televizyon oynatıyor",
            (Self::Paused, Lang::En) => "The TV is paused",
            (Self::Paused, Lang::De) => "Der Fernseher ist pausiert",
            (Self::Paused, Lang::Tr) => "Televizyon duraklatıldı",
            (Self::Stopped, Lang::En) => "The TV is stopped",
            (Self::Stopped, Lang::De) => "Der Fernseher ist gestoppt",
            (Self::Stopped, Lang::Tr) => "Televizyon durduruldu",
            (Self::Idle, Lang::En) => "The TV is on the home screen",
            (Self::Idle, Lang::De) => "Der Fernseher ist im Startbildschirm",
            (Self::Idle, Lang::Tr) => "Televizyon ana ekranda",
            (Self::Buffering, Lang::En) => "The TV is loading",
            (Self::Buffering, Lang::De) => "Der Fernseher lädt",
            (Self::Buffering, Lang::Tr) => "Televizyon yükleniyor",
        }
    }
}

impl Lang {
    /// "The sound is muted" in this language.
    #[must_use]
    const fn sound_muted(self) -> &'static str {
        match self {
            Self::En => "The sound is muted",
            Self::De => "Der Ton ist stumm",
            Self::Tr => "Ses kapalı",
        }
    }
}

impl Display {
    /// A single grandma-friendly status sentence for the whole TV.
    ///
    /// While the TV is off it reads "The TV is off". While on, it leads with
    /// what the TV is doing ("The TV is playing"); if the sound is muted that is
    /// the more important thing to surface, so it reads "The sound is muted".
    #[must_use]
    pub fn status_sentence(&self, lang: Lang) -> &'static str {
        if !self.power().is_on() {
            return self.power().label(lang);
        }
        if self.volume().is_muted() {
            return lang.sound_muted();
        }
        self.playback().label(lang)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::{Display, MediaCommand};
    use crate::source::SourceCatalog;
    use crate::volume::Volume;

    fn on_tv() -> Display {
        Display::powered_on(SourceCatalog::typical_smart_tv(), Volume::new(30))
    }

    #[test]
    fn off_tv_says_off() {
        let tv = Display::new(SourceCatalog::typical_smart_tv(), Volume::new(30));
        assert_eq!(tv.status_sentence(Lang::En), "The TV is off");
        assert_eq!(tv.status_sentence(Lang::De), "Der Fernseher ist aus");
        assert_eq!(tv.status_sentence(Lang::Tr), "Televizyon kapalı");
    }

    #[test]
    fn playing_tv_says_playing() {
        let mut tv = on_tv();
        tv.apply(MediaCommand::LaunchApp("netflix".into())).unwrap();
        tv.apply(MediaCommand::Play).unwrap();
        assert_eq!(tv.status_sentence(Lang::En), "The TV is playing");
        assert_eq!(tv.status_sentence(Lang::Tr), "Televizyon oynatıyor");
    }

    #[test]
    fn muted_tv_surfaces_the_mute() {
        let mut tv = on_tv();
        tv.apply(MediaCommand::LaunchApp("netflix".into())).unwrap();
        tv.apply(MediaCommand::Play).unwrap();
        tv.apply(MediaCommand::SetMute(true)).unwrap();
        assert_eq!(tv.status_sentence(Lang::En), "The sound is muted");
        assert_eq!(tv.status_sentence(Lang::De), "Der Ton ist stumm");
    }

    #[test]
    fn every_power_and_playback_state_has_three_languages() {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            for p in [PowerState::On, PowerState::Off, PowerState::Standby] {
                assert!(!p.label(lang).is_empty());
            }
            for s in [
                PlaybackState::Playing,
                PlaybackState::Paused,
                PlaybackState::Stopped,
                PlaybackState::Idle,
                PlaybackState::Buffering,
            ] {
                assert!(!s.label(lang).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-028: the UI must never surface protocol, vendor or
        // implementation terms — only household words.
        const BANNED: &[&str] = &[
            "CEC", "DLNA", "MQTT", "entity_id", "media_player", "webOS", "WebOS",
            "Tizen", "Cast", "logical address", "WoWLAN", "pod", "kubelet",
            "PlaybackState", "PowerState",
        ];
        let mut texts: Vec<&'static str> = Vec::new();
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            texts.push(lang.sound_muted());
            for p in [PowerState::On, PowerState::Off, PowerState::Standby] {
                texts.push(p.label(lang));
            }
            for s in [
                PlaybackState::Playing,
                PlaybackState::Paused,
                PlaybackState::Stopped,
                PlaybackState::Idle,
                PlaybackState::Buffering,
            ] {
                texts.push(s.label(lang));
            }
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
