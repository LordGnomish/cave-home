// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Grandma-friendly EN / DE / TR descriptions of an `ESPHome` entity kind.
//!
//! cave-home never shows a household the protocol words ("entity", "protobuf",
//! "object id"). It shows a plain noun in the home language — see ADR-007 and
//! Charter §2 persona-1.

use crate::entity::EntityKind;

/// The household's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English.
    En,
    /// German (Deutsch).
    De,
    /// Turkish (Türkçe).
    Tr,
}

impl EntityKind {
    /// A short, jargon-free noun for this kind of device in `lang`.
    // Kept as a full per-(kind, language) table for readability. A few cells
    // legitimately coincide across languages (e.g. "Sensor" in EN and DE), so
    // the same-arm lint does not apply.
    #[allow(clippy::match_same_arms)]
    #[must_use]
    pub const fn describe(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::BinarySensor, Lang::En) => "On/off sensor",
            (Self::BinarySensor, Lang::De) => "Ein/Aus-Sensor",
            (Self::BinarySensor, Lang::Tr) => "Açık/kapalı sensörü",
            (Self::Cover, Lang::En) => "Cover",
            (Self::Cover, Lang::De) => "Rollladen",
            (Self::Cover, Lang::Tr) => "Panjur",
            (Self::Fan, Lang::En) => "Fan",
            (Self::Fan, Lang::De) => "Ventilator",
            (Self::Fan, Lang::Tr) => "Vantilatör",
            (Self::Light, Lang::En) => "Light",
            (Self::Light, Lang::De) => "Licht",
            (Self::Light, Lang::Tr) => "Işık",
            (Self::Sensor, Lang::En) => "Sensor",
            (Self::Sensor, Lang::De) => "Sensor",
            (Self::Sensor, Lang::Tr) => "Sensör",
            (Self::Switch, Lang::En) => "Switch",
            (Self::Switch, Lang::De) => "Schalter",
            (Self::Switch, Lang::Tr) => "Anahtar",
            (Self::TextSensor, Lang::En) => "Text reading",
            (Self::TextSensor, Lang::De) => "Textanzeige",
            (Self::TextSensor, Lang::Tr) => "Metin göstergesi",
        }
    }
}
