// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Portal `/jarvis` page view-model — the voice-assistant status panel.
//!
//! It models whether the assistant is listening, which room microphones are
//! awake, the wake words it answers to, and a short history of recent spoken
//! interactions (who said what, where, and what happened).
//!
//! Like the rest of `cave-home-portal` this is a **pure UI model** — std-only,
//! no network, no dependency on the `cave-home-jarvis` runtime. The assistant
//! feeds it plain facts (a room, a sentence, an outcome); this module turns them
//! into a grandma-friendly, localised page (Charter §6.3).

use crate::label::Lang;

/// How a recent command was understood — shown only as a subtle badge, never as
/// jargon, but useful at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderstoodBy {
    /// Matched directly (the fast, offline path).
    Direct,
    /// Worked out by the local assistant model.
    Assistant,
}

impl UnderstoodBy {
    /// A grandma-friendly, localised badge.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Direct, Lang::En) => "understood",
            (Self::Direct, Lang::De) => "verstanden",
            (Self::Direct, Lang::Tr) => "anlaşıldı",
            (Self::Assistant, Lang::En) => "figured out",
            (Self::Assistant, Lang::De) => "überlegt",
            (Self::Assistant, Lang::Tr) => "düşünüldü",
        }
    }
}

/// The wake/listen state of one room's microphone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicStatus {
    /// The room the microphone sits in (friendly name).
    pub room: String,
    /// Whether it is currently listening for the wake word.
    pub listening: bool,
}

/// One recent spoken interaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interaction {
    /// The room it happened in.
    pub room: String,
    /// The household member, if recognised (`None` = unknown voice).
    pub speaker: Option<String>,
    /// What was said.
    pub said: String,
    /// What the assistant did or answered.
    pub did: String,
    /// How it was understood.
    pub understood_by: UnderstoodBy,
}

/// The voice-assistant status page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JarvisPage {
    /// The wake words the assistant answers to.
    pub wake_words: Vec<String>,
    /// Per-room microphone status.
    pub mics: Vec<MicStatus>,
    /// Recent interactions, newest first.
    pub recent: Vec<Interaction>,
}

impl JarvisPage {
    /// An empty page.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a room microphone (builder-style).
    #[must_use]
    pub fn with_mic(mut self, room: impl Into<String>, listening: bool) -> Self {
        self.mics.push(MicStatus {
            room: room.into(),
            listening,
        });
        self
    }

    /// Add a recent interaction (builder-style; kept newest-first).
    #[must_use]
    pub fn with_interaction(mut self, interaction: Interaction) -> Self {
        self.recent.insert(0, interaction);
        self
    }

    /// The localised page title.
    #[must_use]
    pub const fn title(lang: Lang) -> &'static str {
        match lang {
            Lang::En => "Assistant",
            Lang::De => "Assistent",
            Lang::Tr => "Asistan",
        }
    }

    /// How many rooms are actively listening.
    #[must_use]
    pub fn listening_count(&self) -> usize {
        self.mics.iter().filter(|m| m.listening).count()
    }

    /// Whether the assistant is awake anywhere.
    #[must_use]
    pub fn is_listening(&self) -> bool {
        self.listening_count() > 0
    }

    /// A grandma-friendly one-line status summary.
    #[must_use]
    pub fn summary(&self, lang: Lang) -> String {
        let n = self.listening_count();
        match lang {
            Lang::En if n == 0 => "Not listening right now.".into(),
            Lang::En if n == 1 => "Listening in 1 room.".into(),
            Lang::En => format!("Listening in {n} rooms."),
            Lang::De if n == 0 => "Hört gerade nicht zu.".into(),
            Lang::De if n == 1 => "Hört in 1 Raum zu.".into(),
            Lang::De => format!("Hört in {n} Räumen zu."),
            Lang::Tr if n == 0 => "Şu anda dinlemiyor.".into(),
            Lang::Tr if n == 1 => "1 odada dinliyor.".into(),
            Lang::Tr => format!("{n} odada dinliyor."),
        }
    }

    /// A one-line rendering of an interaction for the history list.
    #[must_use]
    pub fn render_interaction(interaction: &Interaction, lang: Lang) -> String {
        let who = interaction.speaker.as_deref().unwrap_or(match lang {
            Lang::En => "Someone",
            Lang::De => "Jemand",
            Lang::Tr => "Biri",
        });
        let connector = match lang {
            Lang::En => "in the",
            Lang::De => "im",
            Lang::Tr => "—",
        };
        format!(
            "{who} {connector} {}: \"{}\" → {}",
            interaction.room, interaction.said, interaction.did
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> JarvisPage {
        JarvisPage {
            wake_words: vec!["jarvis".into()],
            ..JarvisPage::new()
        }
        .with_mic("kitchen", true)
        .with_mic("office", true)
        .with_mic("garage", false)
        .with_interaction(Interaction {
            room: "kitchen".into(),
            speaker: Some("Burak".into()),
            said: "turn the kitchen light on".into(),
            did: "Turned on the kitchen light.".into(),
            understood_by: UnderstoodBy::Direct,
        })
    }

    #[test]
    fn title_is_localised_and_jargon_free() {
        assert_eq!(JarvisPage::title(Lang::En), "Assistant");
        assert_eq!(JarvisPage::title(Lang::De), "Assistent");
        assert_eq!(JarvisPage::title(Lang::Tr), "Asistan");
    }

    #[test]
    fn counts_listening_rooms() {
        let p = page();
        assert_eq!(p.listening_count(), 2);
        assert!(p.is_listening());
    }

    #[test]
    fn summary_reflects_room_count() {
        assert_eq!(page().summary(Lang::En), "Listening in 2 rooms.");
        assert_eq!(JarvisPage::new().summary(Lang::En), "Not listening right now.");
        assert_eq!(
            JarvisPage::new().with_mic("den", true).summary(Lang::En),
            "Listening in 1 room."
        );
    }

    #[test]
    fn summary_localised() {
        assert!(page().summary(Lang::De).contains("Räumen"));
        assert!(page().summary(Lang::Tr).contains("odada"));
    }

    #[test]
    fn newest_interaction_is_first() {
        let p = page().with_interaction(Interaction {
            room: "office".into(),
            speaker: None,
            said: "what's the temperature".into(),
            did: "It's 21 degrees.".into(),
            understood_by: UnderstoodBy::Assistant,
        });
        assert_eq!(p.recent[0].room, "office");
        assert_eq!(p.recent[0].speaker, None);
    }

    #[test]
    fn renders_interaction_with_speaker_and_outcome() {
        let p = page();
        let line = JarvisPage::render_interaction(&p.recent[0], Lang::En);
        assert!(line.contains("Burak"));
        assert!(line.contains("kitchen"));
        assert!(line.contains("Turned on the kitchen light."));
    }

    #[test]
    fn renders_unknown_speaker_friendly() {
        let i = Interaction {
            room: "hall".into(),
            speaker: None,
            said: "hello".into(),
            did: "Hi there.".into(),
            understood_by: UnderstoodBy::Direct,
        };
        assert!(JarvisPage::render_interaction(&i, Lang::En).starts_with("Someone"));
        assert!(JarvisPage::render_interaction(&i, Lang::Tr).starts_with("Biri"));
    }

    #[test]
    fn understood_by_badges_localised() {
        assert_eq!(UnderstoodBy::Direct.label(Lang::En), "understood");
        assert_eq!(UnderstoodBy::Assistant.label(Lang::Tr), "düşünüldü");
    }
}
