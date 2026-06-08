//! Grandma-friendly, localised labels for covers (Charter §6.3, ADR-007).
//!
//! The household never sees "set_position", "RTS rolling code" or a raw
//! percentage in prose. They see "The garage door is open", "Die Jalousie ist
//! halb offen", "Salon panjuru kapalı". This module turns a device class plus a
//! [`CoverState`] (and openness) into that sentence, in EN / DE / TR — the
//! Charter §6.3 mandatory languages from M1.

use crate::device_class::DeviceClass;
use crate::position::Position;
use crate::state::CoverState;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl DeviceClass {
    /// The household name for this kind of cover (singular, lower-case so it
    /// can be slotted into a sentence). No protocol or vendor terms.
    #[must_use]
    pub const fn noun(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Garage, Lang::En) => "garage door",
            (Self::Garage, Lang::De) => "Garagentor",
            (Self::Garage, Lang::Tr) => "garaj kapısı",
            (Self::Blind, Lang::En) => "blinds",
            (Self::Blind, Lang::De) => "Jalousie",
            (Self::Blind, Lang::Tr) => "panjur",
            (Self::Shade, Lang::En) => "shade",
            (Self::Shade, Lang::De) => "Rollo",
            (Self::Shade, Lang::Tr) => "stor perde",
            (Self::Awning, Lang::En) => "awning",
            (Self::Awning, Lang::De) => "Markise",
            (Self::Awning, Lang::Tr) => "tente",
            (Self::Curtain, Lang::En) => "curtains",
            (Self::Curtain, Lang::De) => "Vorhang",
            (Self::Curtain, Lang::Tr) => "perde",
            (Self::Shutter, Lang::En) => "shutters",
            (Self::Shutter, Lang::De) => "Rollladen",
            (Self::Shutter, Lang::Tr) => "kepenk",
            (Self::Gate, Lang::En) => "gate",
            (Self::Gate, Lang::De) => "Tor",
            (Self::Gate, Lang::Tr) => "bahçe kapısı",
            (Self::Door, Lang::En) => "door",
            (Self::Door, Lang::De) => "Tür",
            (Self::Door, Lang::Tr) => "kapı",
            (Self::Window, Lang::En) => "window",
            (Self::Window, Lang::De) => "Fenster",
            (Self::Window, Lang::Tr) => "pencere",
        }
    }
}

/// A coarse openness band used only to phrase a part-way cover ("a little",
/// "half", "mostly") so the household never has to read a percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Openness {
    Closed,
    ABitOpen,
    HalfOpen,
    MostlyOpen,
    Open,
}

impl Openness {
    const fn of(position: Position) -> Self {
        match position.percent() {
            0 => Self::Closed,
            1..=33 => Self::ABitOpen,
            34..=66 => Self::HalfOpen,
            67..=99 => Self::MostlyOpen,
            _ => Self::Open,
        }
    }

    /// The bare openness adjective (no copula): "half open", "halb offen",
    /// "yarı açık".
    const fn phrase(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Closed, Lang::En) => "closed",
            (Self::Closed, Lang::De) => "geschlossen",
            (Self::Closed, Lang::Tr) => "kapalı",
            (Self::ABitOpen, Lang::En) => "open a little",
            (Self::ABitOpen, Lang::De) => "ein wenig offen",
            (Self::ABitOpen, Lang::Tr) => "biraz açık",
            (Self::HalfOpen, Lang::En) => "half open",
            (Self::HalfOpen, Lang::De) => "halb offen",
            (Self::HalfOpen, Lang::Tr) => "yarı açık",
            (Self::MostlyOpen, Lang::En) => "mostly open",
            (Self::MostlyOpen, Lang::De) => "fast ganz offen",
            (Self::MostlyOpen, Lang::Tr) => "neredeyse tamamen açık",
            (Self::Open, Lang::En) => "open",
            (Self::Open, Lang::De) => "offen",
            (Self::Open, Lang::Tr) => "açık",
        }
    }

    /// The complete at-rest predicate, with the EN/DE copula attached: "is half
    /// open", "ist halb offen". Turkish needs no copula, so the bare phrase is
    /// the predicate.
    const fn predicate(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Closed, Lang::En) => "is closed",
            (Self::Closed, Lang::De) => "ist geschlossen",
            (Self::ABitOpen, Lang::En) => "is open a little",
            (Self::ABitOpen, Lang::De) => "ist ein wenig offen",
            (Self::HalfOpen, Lang::En) => "is half open",
            (Self::HalfOpen, Lang::De) => "ist halb offen",
            (Self::MostlyOpen, Lang::En) => "is mostly open",
            (Self::MostlyOpen, Lang::De) => "ist fast ganz offen",
            (Self::Open, Lang::En) => "is open",
            (Self::Open, Lang::De) => "ist offen",
            (_, Lang::Tr) => self.phrase(Lang::Tr),
        }
    }
}

/// A localised, jargon-free status sentence for a cover.
///
/// Combines the device-class noun with a phrase derived from the motion state
/// and (for an at-rest part-way cover) its openness band. Examples:
/// "The garage door is open." / "Die Jalousie ist halb offen." / "Salon
/// panjuru kapanıyor."
///
/// `position` is consulted only when the cover is at rest and `Stopped`
/// part-way, to choose between "a little / half / mostly open".
#[must_use]
pub fn status_sentence(
    class: DeviceClass,
    state: CoverState,
    position: Position,
    lang: Lang,
) -> String {
    let noun = class.noun(lang);
    // `predicate` is a complete localised predicate (verb included), so the
    // sentence builder only has to slot in the noun. The at-rest states reuse
    // the openness words; the moving states are full verb phrases. German is
    // handled here so a verb phrase ("öffnet sich") is not forced through an
    // "ist …" frame.
    let predicate = match state {
        CoverState::Open => Openness::Open.predicate(lang),
        CoverState::Closed => Openness::Closed.predicate(lang),
        CoverState::Stopped => Openness::of(position).predicate(lang),
        CoverState::Opening => match lang {
            // Moving states are verb phrases, not "is/ist <adjective>".
            Lang::En => "is opening",
            Lang::De => "öffnet sich",
            Lang::Tr => "açılıyor",
        },
        CoverState::Closing => match lang {
            Lang::En => "is closing",
            Lang::De => "schließt sich",
            Lang::Tr => "kapanıyor",
        },
    };
    match lang {
        // English / German lead with an article + noun; Turkish does not.
        Lang::En => format!("The {noun} {predicate}."),
        Lang::De => format!("Das/Die {noun} {predicate}."),
        Lang::Tr => format!("{noun} {predicate}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(p: u8) -> Position {
        Position::new(p).expect("valid test position")
    }

    #[test]
    fn every_class_has_three_language_nouns() {
        for class in [
            DeviceClass::Garage,
            DeviceClass::Blind,
            DeviceClass::Shade,
            DeviceClass::Awning,
            DeviceClass::Curtain,
            DeviceClass::Shutter,
            DeviceClass::Gate,
            DeviceClass::Door,
            DeviceClass::Window,
        ] {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!class.noun(lang).is_empty());
            }
        }
    }

    #[test]
    fn open_and_closed_sentences_read_naturally() {
        assert_eq!(
            status_sentence(DeviceClass::Garage, CoverState::Open, Position::OPEN, Lang::En),
            "The garage door is open."
        );
        assert_eq!(
            status_sentence(DeviceClass::Garage, CoverState::Closed, Position::CLOSED, Lang::En),
            "The garage door is closed."
        );
    }

    #[test]
    fn partway_cover_uses_coarse_openness_words_not_numbers() {
        let s = status_sentence(DeviceClass::Blind, CoverState::Stopped, pos(50), Lang::En);
        assert!(s.contains("half open"), "got: {s}");
        // No digits leak into the user-facing string.
        assert!(!s.chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn openness_bands_cover_the_range() {
        assert_eq!(Openness::of(pos(0)), Openness::Closed);
        assert_eq!(Openness::of(pos(10)), Openness::ABitOpen);
        assert_eq!(Openness::of(pos(50)), Openness::HalfOpen);
        assert_eq!(Openness::of(pos(80)), Openness::MostlyOpen);
        assert_eq!(Openness::of(pos(100)), Openness::Open);
    }

    #[test]
    fn moving_states_localised() {
        assert_eq!(
            status_sentence(DeviceClass::Shutter, CoverState::Closing, pos(40), Lang::Tr),
            "kepenk kapanıyor."
        );
        assert_eq!(
            status_sentence(DeviceClass::Awning, CoverState::Opening, pos(20), Lang::De),
            "Das/Die Markise öffnet sich."
        );
        assert_eq!(
            status_sentence(DeviceClass::Blind, CoverState::Stopped, pos(50), Lang::De),
            "Das/Die Jalousie ist halb offen."
        );
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol/vendor/impl terms.
        const BANNED: &[&str] = &[
            "set_position",
            "SetPosition",
            "RTS",
            "Somfy",
            "OpenGarage",
            "ESPHome",
            "MQTT",
            "Zigbee",
            "Z-Wave",
            "Matter",
            "entity_id",
            "actuator",
            "endpoint",
            "rolling code",
            "pod",
            "kubelet",
            "%",
        ];
        for class in [
            DeviceClass::Garage,
            DeviceClass::Blind,
            DeviceClass::Shade,
            DeviceClass::Awning,
            DeviceClass::Curtain,
            DeviceClass::Shutter,
            DeviceClass::Gate,
            DeviceClass::Door,
            DeviceClass::Window,
        ] {
            for state in [
                CoverState::Open,
                CoverState::Closed,
                CoverState::Opening,
                CoverState::Closing,
                CoverState::Stopped,
            ] {
                for p in [pos(0), pos(20), pos(50), pos(80), pos(100)] {
                    for lang in [Lang::En, Lang::De, Lang::Tr] {
                        let text = status_sentence(class, state, p, lang);
                        for banned in BANNED {
                            assert!(
                                !text.contains(banned),
                                "{class:?}/{state:?} leaks jargon {banned:?}: {text}"
                            );
                        }
                    }
                }
            }
        }
    }
}
