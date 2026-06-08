// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Grandma-friendly, localized descriptions of KNX actions (Charter §6.3,
//! ADR-007).
//!
//! Nothing technical reaches the household. A group address (`"1/2/3"`), a
//! datapoint type (`"DPT 9.001"`), an individual address (`"1.1.5"`) — none of
//! it is ever shown. Instead we turn a decoded action into a plain sentence in
//! EN / DE / TR (the Charter §6.3 mandatory languages): "Living-room light on",
//! "Blinds going down", "Temperature 21°".
//!
//! These are *home-world* words — "light", "blinds", "temperature" — explicitly
//! blessed by the charter; protocol terms are not.

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// A household-meaningful action decoded from a telegram, ready to describe.
///
/// The caller maps a decoded datapoint onto one of these (a switch becomes
/// `Light`, a dimming command becomes `Blinds` or `Light` depending on the
/// installed device, a temperature datapoint becomes `Temperature`). This enum
/// is deliberately free of any KNX terminology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A light turned on or off.
    Light {
        /// `true` = on.
        on: bool,
    },
    /// Blinds moving up or down.
    Blinds {
        /// `true` = going up.
        up: bool,
    },
    /// A temperature reading or set-point, in whole degrees for display.
    Temperature {
        /// Degrees Celsius (rounded for the household; we never show decimals).
        celsius: i32,
    },
}

impl Action {
    /// A plain, localized sentence describing the action.
    #[must_use]
    pub fn describe(self, lang: Lang) -> String {
        match self {
            Self::Light { on } => light_phrase(on, lang).to_owned(),
            Self::Blinds { up } => blinds_phrase(up, lang).to_owned(),
            Self::Temperature { celsius } => match lang {
                Lang::En => format!("Temperature {celsius}°"),
                Lang::De => format!("Temperatur {celsius}°"),
                Lang::Tr => format!("Sıcaklık {celsius}°"),
            },
        }
    }
}

const fn light_phrase(on: bool, lang: Lang) -> &'static str {
    match (on, lang) {
        (true, Lang::En) => "Light on",
        (true, Lang::De) => "Licht an",
        (true, Lang::Tr) => "Işık açık",
        (false, Lang::En) => "Light off",
        (false, Lang::De) => "Licht aus",
        (false, Lang::Tr) => "Işık kapalı",
    }
}

const fn blinds_phrase(up: bool, lang: Lang) -> &'static str {
    match (up, lang) {
        (true, Lang::En) => "Blinds going up",
        (true, Lang::De) => "Jalousie fährt hoch",
        (true, Lang::Tr) => "Panjur yukarı çıkıyor",
        (false, Lang::En) => "Blinds going down",
        (false, Lang::De) => "Jalousie fährt runter",
        (false, Lang::Tr) => "Panjur aşağı iniyor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_on_off_localized() {
        assert_eq!(Action::Light { on: true }.describe(Lang::En), "Light on");
        assert_eq!(Action::Light { on: false }.describe(Lang::De), "Licht aus");
        assert_eq!(Action::Light { on: true }.describe(Lang::Tr), "Işık açık");
    }

    #[test]
    fn blinds_direction_localized() {
        assert_eq!(
            Action::Blinds { up: false }.describe(Lang::En),
            "Blinds going down"
        );
        assert!(Action::Blinds { up: true }.describe(Lang::De).contains("hoch"));
        assert!(Action::Blinds { up: false }
            .describe(Lang::Tr)
            .contains("aşağı"));
    }

    #[test]
    fn temperature_weaves_in_degrees() {
        assert_eq!(
            Action::Temperature { celsius: 21 }.describe(Lang::En),
            "Temperature 21°"
        );
        assert_eq!(
            Action::Temperature { celsius: 21 }.describe(Lang::Tr),
            "Sıcaklık 21°"
        );
    }

    #[test]
    fn every_action_has_all_three_languages() {
        let actions = [
            Action::Light { on: true },
            Action::Light { on: false },
            Action::Blinds { up: true },
            Action::Blinds { up: false },
            Action::Temperature { celsius: 0 },
        ];
        for a in actions {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!a.describe(lang).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol/decode terms.
        const BANNED: &[&str] = &[
            "DPT", "APCI", "cEMI", "KNX", "group address", "1/2/3", "1.1.5",
            "MQTT", "Zigbee", "telegram", "datapoint", "0x", "byte",
        ];
        let actions = [
            Action::Light { on: true },
            Action::Light { on: false },
            Action::Blinds { up: true },
            Action::Blinds { up: false },
            Action::Temperature { celsius: 21 },
            Action::Temperature { celsius: -5 },
        ];
        for a in actions {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let text = a.describe(lang);
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "action {a:?} leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
