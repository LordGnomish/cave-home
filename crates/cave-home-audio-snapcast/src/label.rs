//! Grandma-friendly, localised wording for what the speakers are doing.
//!
//! The control-plane internals (client ids, group ids, stream codecs, latency
//! milliseconds, JSON-RPC method names) never reach the household. The Portal
//! and voice replies speak in plain language: "Kitchen and living room playing
//! together", "Bedroom speaker muted", "Music in every room". This module owns
//! the [`Lang`] enum and the small, reusable localisation helpers (Charter
//! §6.3, ADR-007 / ADR-020). No protocol jargon is permitted in any string
//! produced here.

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English.
    En,
    /// German.
    De,
    /// Turkish.
    Tr,
}

/// Pick one of three pre-translated words by language. The building block for
/// every localised phrase in the crate.
#[must_use]
pub const fn tr_word(
    lang: Lang,
    en: &'static str,
    de: &'static str,
    tr: &'static str,
) -> &'static str {
    match lang {
        Lang::En => en,
        Lang::De => de,
        Lang::Tr => tr,
    }
}

/// "muted" in the requested language — used for a single speaker.
#[must_use]
pub const fn muted_word(lang: Lang) -> &'static str {
    tr_word(lang, "muted", "stumm", "sessizde")
}

/// "playing" in the requested language.
#[must_use]
pub const fn playing_word(lang: Lang) -> &'static str {
    tr_word(lang, "playing", "spielt", "çalıyor")
}

/// "every room" in the requested language — the headline phrase for whole-house
/// audio.
#[must_use]
pub const fn every_room(lang: Lang) -> &'static str {
    tr_word(lang, "every room", "jedem Raum", "her oda")
}

/// Join a list of names into a household-friendly conjunction:
/// "Kitchen", "Kitchen and living room", "Kitchen, living room and bedroom".
#[must_use]
pub fn join_names(names: &[&str], lang: Lang) -> String {
    let and = tr_word(lang, "and", "und", "ve");
    match names {
        [] => String::new(),
        [only] => (*only).to_string(),
        [first, last] => format!("{first} {and} {last}"),
        [head @ .., last] => {
            let joined = head.join(", ");
            format!("{joined} {and} {last}")
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn join_names_reads_naturally() {
        assert_eq!(join_names(&[], Lang::En), "");
        assert_eq!(join_names(&["Kitchen"], Lang::En), "Kitchen");
        assert_eq!(
            join_names(&["Kitchen", "Living room"], Lang::En),
            "Kitchen and Living room"
        );
        assert_eq!(
            join_names(&["Kitchen", "Living room", "Bedroom"], Lang::En),
            "Kitchen, Living room and Bedroom"
        );
    }

    #[test]
    fn join_names_localises_the_conjunction() {
        assert_eq!(
            join_names(&["Küche", "Wohnzimmer"], Lang::De),
            "Küche und Wohnzimmer"
        );
        assert_eq!(
            join_names(&["Mutfak", "Salon"], Lang::Tr),
            "Mutfak ve Salon"
        );
    }

    #[test]
    fn words_present_in_all_languages() {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            assert!(!muted_word(lang).is_empty());
            assert!(!playing_word(lang).is_empty());
            assert!(!every_room(lang).is_empty());
        }
    }
}
