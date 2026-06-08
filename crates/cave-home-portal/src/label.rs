// SPDX-License-Identifier: Apache-2.0
//! UI language + the grandma-friendly label vocabulary (Charter §6.3, ADR-007).
//!
//! Everything a resident reads on a tile flows through here. The normative
//! translation matrix lives in `docs/ui-language.md`; this module is the typed,
//! tested realisation of the "home-world vocabulary" half of that matrix for the
//! Portal view-model. No implementation term (entity id, MQTT, pod, Zigbee
//! channel, …) is ever allowed to reach one of these strings.

/// A UI language. Charter §6.3 requires English + German + Turkish from M1.
///
/// Turkish is the primary developer locale (see `docs/ui-language.md`), English
/// is the OSS-launch baseline, German is mandatory for the founder's mixed-
/// language household.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    /// English (`en-US`).
    En,
    /// German (`de-DE`).
    De,
    /// Turkish (`tr-TR`).
    Tr,
}

impl Lang {
    /// Every language the Portal must render at M1. Useful for exhaustive tests
    /// and for building a language picker.
    pub const ALL: [Self; 3] = [Self::En, Self::De, Self::Tr];

    /// The IETF-ish tag, for the `lang=` attribute the (deferred) frontend sets.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::En => "en-US",
            Self::De => "de-DE",
            Self::Tr => "tr-TR",
        }
    }

    /// The endonym shown in the language picker ("English", "Deutsch", "Türkçe").
    #[must_use]
    pub const fn endonym(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::De => "Deutsch",
            Self::Tr => "Türkçe",
        }
    }
}

/// A small, closed catalogue of generic UI strings the view-model needs.
///
/// These are not tied to a specific device domain (button captions, status
/// words, the "no devices yet" empty state, …). Domain-specific wording lives
/// next to the thing it describes (e.g. [`crate::viewmodel`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phrase {
    /// The word "On" for a tile that is currently active.
    On,
    /// The word "Off".
    Off,
    /// "Open" — a door/cover/window.
    Open,
    /// "Closed".
    Closed,
    /// "Locked".
    Locked,
    /// "Unlocked".
    Unlocked,
    /// "Unavailable" — the device has not reported recently.
    Unavailable,
    /// A button caption: "Turn on".
    ActionTurnOn,
    /// A button caption: "Turn off".
    ActionTurnOff,
    /// A button caption: "Open".
    ActionOpen,
    /// A button caption: "Close".
    ActionClose,
    /// A button caption: "Lock".
    ActionLock,
    /// A button caption: "Unlock".
    ActionUnlock,
    /// A button caption: "Run" (a scene).
    ActionRun,
    /// Empty-state copy when a home has no rooms/devices yet.
    EmptyHome,
    /// Heading for the favourites strip.
    Favorites,
    /// Heading for the scenes/quick-actions strip.
    Scenes,
    /// The Settings entry that toggles the power-user surface.
    DeveloperView,
}

impl Phrase {
    /// Localised text. Strictly home-world vocabulary — verified jargon-free by
    /// [`tests::ui_strings_carry_no_implementation_jargon`].
    // Some phrases legitimately share a word in a given language (e.g. a cover
    // that is "Open" and a light that is "On" are both "Açık" in Turkish); the
    // arms are distinct on purpose, so silence the identical-arm lint.
    #[allow(clippy::match_same_arms)]
    #[must_use]
    pub const fn text(self, lang: Lang) -> &'static str {
        use Lang::{De, En, Tr};
        use Phrase::{
            ActionClose, ActionLock, ActionOpen, ActionRun, ActionTurnOff, ActionTurnOn,
            ActionUnlock, Closed, DeveloperView, EmptyHome, Favorites, Locked, Off, On, Open,
            Scenes, Unavailable, Unlocked,
        };
        match (self, lang) {
            (On, En) => "On",
            (On, De) => "An",
            (On, Tr) => "Açık",
            (Off, En) => "Off",
            (Off, De) => "Aus",
            (Off, Tr) => "Kapalı",
            (Open, En) => "Open",
            (Open, De) => "Offen",
            (Open, Tr) => "Açık",
            (Closed, En) => "Closed",
            (Closed, De) => "Geschlossen",
            (Closed, Tr) => "Kapalı",
            (Locked, En) => "Locked",
            (Locked, De) => "Verriegelt",
            (Locked, Tr) => "Kilitli",
            (Unlocked, En) => "Unlocked",
            (Unlocked, De) => "Entriegelt",
            (Unlocked, Tr) => "Kilit açık",
            (Unavailable, En) => "Not responding",
            (Unavailable, De) => "Keine Antwort",
            (Unavailable, Tr) => "Yanıt vermiyor",
            (ActionTurnOn, En) => "Turn on",
            (ActionTurnOn, De) => "Einschalten",
            (ActionTurnOn, Tr) => "Aç",
            (ActionTurnOff, En) => "Turn off",
            (ActionTurnOff, De) => "Ausschalten",
            (ActionTurnOff, Tr) => "Kapat",
            (ActionOpen, En) => "Open",
            (ActionOpen, De) => "Öffnen",
            (ActionOpen, Tr) => "Aç",
            (ActionClose, En) => "Close",
            (ActionClose, De) => "Schließen",
            (ActionClose, Tr) => "Kapat",
            (ActionLock, En) => "Lock",
            (ActionLock, De) => "Verriegeln",
            (ActionLock, Tr) => "Kilitle",
            (ActionUnlock, En) => "Unlock",
            (ActionUnlock, De) => "Entriegeln",
            (ActionUnlock, Tr) => "Kilidi aç",
            (ActionRun, En) => "Run",
            (ActionRun, De) => "Starten",
            (ActionRun, Tr) => "Başlat",
            (EmptyHome, En) => "No rooms yet. Add your first device to get started.",
            (EmptyHome, De) => "Noch keine Räume. Füge dein erstes Gerät hinzu.",
            (EmptyHome, Tr) => "Henüz oda yok. İlk cihazını ekleyerek başla.",
            (Favorites, En) => "Favourites",
            (Favorites, De) => "Favoriten",
            (Favorites, Tr) => "Sık kullanılanlar",
            (Scenes, En) => "Scenes",
            (Scenes, De) => "Szenen",
            (Scenes, Tr) => "Sahneler",
            (DeveloperView, En) => "Developer view",
            (DeveloperView, De) => "Entwickleransicht",
            (DeveloperView, Tr) => "Geliştirici görünümü",
        }
    }
}

/// The list of every [`Phrase`], for exhaustive i18n / jargon tests.
#[cfg(test)]
pub(crate) const ALL_PHRASES: &[Phrase] = &[
    Phrase::On,
    Phrase::Off,
    Phrase::Open,
    Phrase::Closed,
    Phrase::Locked,
    Phrase::Unlocked,
    Phrase::Unavailable,
    Phrase::ActionTurnOn,
    Phrase::ActionTurnOff,
    Phrase::ActionOpen,
    Phrase::ActionClose,
    Phrase::ActionLock,
    Phrase::ActionUnlock,
    Phrase::ActionRun,
    Phrase::EmptyHome,
    Phrase::Favorites,
    Phrase::Scenes,
    Phrase::DeveloperView,
];

/// The banned-token list shared by every jargon test in the crate. These are
/// implementation terms (Charter §6.3 / `docs/ui-language.md` "never show")
/// that must never appear in any resident-facing string.
// NOTE: every entry below is jargon-detection *test data* — the very terms the
// resident-facing UI must NEVER contain. They are only ever compared against, so
// tools/gates.sh G8 flagging a few of them here is an expected false positive on
// the detector's own dictionary (G8 stays a warning; the gate still passes).
#[cfg(test)]
pub(crate) const BANNED_JARGON: &[&str] = &[
    "entity_id",
    "entity id",
    "MQTT",
    "mqtt",
    "Zigbee",
    "Z-Wave",
    "PAN-ID",
    "pod",
    "kubelet",
    "apiserver",
    "etcd",
    "namespace",
    "RBAC",
    "Helm",
    "YAML",
    "JSON",
    "container",
    "Kubernetes",
    "K3s",
    "VXLAN",
    "DPT ",
    "SysAP",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phrase_has_three_non_empty_translations() {
        for &p in ALL_PHRASES {
            for lang in Lang::ALL {
                assert!(
                    !p.text(lang).trim().is_empty(),
                    "phrase {p:?} missing text for {lang:?}"
                );
            }
        }
    }

    #[test]
    fn languages_have_distinct_tags_and_endonyms() {
        let tags: Vec<_> = Lang::ALL.iter().map(|l| l.tag()).collect();
        assert_eq!(tags, ["en-US", "de-DE", "tr-TR"]);
        for lang in Lang::ALL {
            assert!(!lang.endonym().is_empty());
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the resident-facing UI must never surface protocol,
        // cluster, or wire-format terms. (Copied in spirit from
        // cave-home-air-quality::category::tests.)
        for &p in ALL_PHRASES {
            for lang in Lang::ALL {
                let text = p.text(lang);
                for banned in BANNED_JARGON {
                    assert!(
                        !text.contains(banned),
                        "phrase {p:?} ({lang:?}) leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
