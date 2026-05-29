// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Grandma-friendly, localised action phrases (Charter §6.3, ADR-007).
//!
//! Nothing a household sees mentions a serial, a channel, a pairing ID or
//! "datapoint". A free@home action becomes a plain sentence in the user's
//! language: "Living-room light on", "Bedroom blind at 50 %", "Hallway scene
//! activated". This module owns those phrases for EN / DE / TR (the Charter
//! §6.3 mandatory languages).

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// A household-level action, used to build a confirmation phrase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A light or socket turned on.
    On,
    /// A light or socket turned off.
    Off,
    /// A light dimmed to a percentage.
    Brightness(u8),
    /// A blind opened.
    Open,
    /// A blind closed.
    Closed,
    /// A blind moved to a percentage.
    BlindPosition(u8),
    /// A thermostat set to a target temperature (whole degrees).
    Temperature(i16),
    /// A scene activated.
    SceneActivated,
}

/// Build the localised confirmation phrase for an action on a named place /
/// device (e.g. "Living room", "Bedroom"). The place name is supplied by the
/// caller already in the user's language.
#[must_use]
pub fn action_phrase(lang: Lang, place: &str, action: Action) -> String {
    match (lang, action) {
        // English
        (Lang::En, Action::On) => format!("{place} light on"),
        (Lang::En, Action::Off) => format!("{place} light off"),
        (Lang::En, Action::Brightness(p)) => format!("{place} at {p} %"),
        (Lang::En, Action::Open) => format!("{place} blind open"),
        (Lang::En, Action::Closed) => format!("{place} blind closed"),
        (Lang::En, Action::BlindPosition(p)) => format!("{place} blind at {p} %"),
        (Lang::En, Action::Temperature(t)) => format!("{place} set to {t} °C"),
        (Lang::En, Action::SceneActivated) => format!("{place} scene activated"),
        // German
        (Lang::De, Action::On) => format!("{place} Licht an"),
        (Lang::De, Action::Off) => format!("{place} Licht aus"),
        (Lang::De, Action::Brightness(p)) => format!("{place} auf {p} %"),
        (Lang::De, Action::Open) => format!("{place} Rollladen offen"),
        (Lang::De, Action::Closed) => format!("{place} Rollladen geschlossen"),
        (Lang::De, Action::BlindPosition(p)) => format!("{place} Rollladen auf {p} %"),
        (Lang::De, Action::Temperature(t)) => format!("{place} auf {t} °C eingestellt"),
        (Lang::De, Action::SceneActivated) => format!("Szene {place} aktiviert"),
        // Turkish
        (Lang::Tr, Action::On) => format!("{place} ışık açık"),
        (Lang::Tr, Action::Off) => format!("{place} ışık kapalı"),
        (Lang::Tr, Action::Brightness(p)) => format!("{place} %{p}"),
        (Lang::Tr, Action::Open) => format!("{place} perde açık"),
        (Lang::Tr, Action::Closed) => format!("{place} perde kapalı"),
        (Lang::Tr, Action::BlindPosition(p)) => format!("{place} perde %{p}"),
        (Lang::Tr, Action::Temperature(t)) => format!("{place} {t} °C ayarlandı"),
        (Lang::Tr, Action::SceneActivated) => format!("{place} sahnesi etkin"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_phrases() {
        assert_eq!(
            action_phrase(Lang::En, "Living room", Action::On),
            "Living room light on"
        );
        assert_eq!(
            action_phrase(Lang::En, "Bedroom", Action::BlindPosition(50)),
            "Bedroom blind at 50 %"
        );
        assert_eq!(
            action_phrase(Lang::En, "Hallway", Action::SceneActivated),
            "Hallway scene activated"
        );
    }

    #[test]
    fn german_phrases() {
        assert_eq!(
            action_phrase(Lang::De, "Wohnzimmer", Action::Brightness(50)),
            "Wohnzimmer auf 50 %"
        );
        assert_eq!(
            action_phrase(Lang::De, "Schlafzimmer", Action::Closed),
            "Schlafzimmer Rollladen geschlossen"
        );
    }

    #[test]
    fn turkish_phrases() {
        assert_eq!(
            action_phrase(Lang::Tr, "Salon", Action::On),
            "Salon ışık açık"
        );
        assert_eq!(
            action_phrase(Lang::Tr, "Mutfak", Action::BlindPosition(75)),
            "Mutfak perde %75"
        );
    }

    #[test]
    fn every_action_has_all_three_languages() {
        let actions = [
            Action::On,
            Action::Off,
            Action::Brightness(30),
            Action::Open,
            Action::Closed,
            Action::BlindPosition(40),
            Action::Temperature(21),
            Action::SceneActivated,
        ];
        for a in actions {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!action_phrase(lang, "Room", a).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: confirmation phrases must never surface protocol terms.
        const BANNED: &[&str] = &[
            "datapoint",
            "pairing",
            "odp",
            "idp",
            "SysAP",
            "System Access Point",
            "KNX",
            "MQTT",
            "channel",
            "functionID",
            "entity_id",
            "ABB",
            "ch0",
        ];
        let actions = [
            Action::On,
            Action::Off,
            Action::Brightness(50),
            Action::Open,
            Action::Closed,
            Action::BlindPosition(50),
            Action::Temperature(21),
            Action::SceneActivated,
        ];
        for a in actions {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                // Use a neutral place name so we only test the generated text.
                let text = action_phrase(lang, "Room", a);
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "phrase {a:?}/{lang:?} leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
