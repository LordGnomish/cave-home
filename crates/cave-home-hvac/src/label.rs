//! Grandma-friendly, localised labels for what the climate system is doing
//! (Charter §6.3, ADR-007, ADR-012).
//!
//! The end-user never sees `HeatCool`, `hvac_action`, or a vendor endpoint
//! name. They see "Heating to 21°", "Reached your temperature", "Away" — in
//! EN / DE / TR (the Charter §6.3 mandatory languages from M1). This module is
//! the single place those strings live.

use crate::mode::{FanMode, HvacAction, HvacMode, PresetMode};

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl HvacMode {
    /// Plain-language name for the requested mode.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Off, Lang::En) => "Off",
            (Self::Off, Lang::De) => "Aus",
            (Self::Off, Lang::Tr) => "Kapalı",
            (Self::Heat, Lang::En) => "Heating",
            (Self::Heat, Lang::De) => "Heizen",
            (Self::Heat, Lang::Tr) => "Isıtma",
            (Self::Cool, Lang::En) => "Cooling",
            (Self::Cool, Lang::De) => "Kühlen",
            (Self::Cool, Lang::Tr) => "Soğutma",
            (Self::HeatCool, Lang::En) => "Keep comfortable",
            (Self::HeatCool, Lang::De) => "Angenehm halten",
            (Self::HeatCool, Lang::Tr) => "Konforlu tut",
            (Self::Auto, Lang::En) => "Automatic",
            (Self::Auto, Lang::De) => "Automatisch",
            (Self::Auto, Lang::Tr) => "Otomatik",
            (Self::Dry, Lang::En) => "Drying the air",
            (Self::Dry, Lang::De) => "Luft trocknen",
            (Self::Dry, Lang::Tr) => "Havayı kurutma",
            (Self::FanOnly, Lang::En) => "Fan only",
            (Self::FanOnly, Lang::De) => "Nur Lüfter",
            (Self::FanOnly, Lang::Tr) => "Sadece fan",
        }
    }
}

impl HvacAction {
    /// Plain-language description of what is happening right now.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Off, Lang::En) => "Off",
            (Self::Off, Lang::De) => "Aus",
            (Self::Off, Lang::Tr) => "Kapalı",
            (Self::Idle, Lang::En) => "Reached your temperature",
            (Self::Idle, Lang::De) => "Wunschtemperatur erreicht",
            (Self::Idle, Lang::Tr) => "İstediğiniz sıcaklığa ulaşıldı",
            (Self::Heating, Lang::En) => "Heating",
            (Self::Heating, Lang::De) => "Heizt",
            (Self::Heating, Lang::Tr) => "Isıtıyor",
            (Self::Cooling, Lang::En) => "Cooling",
            (Self::Cooling, Lang::De) => "Kühlt",
            (Self::Cooling, Lang::Tr) => "Soğutuyor",
            (Self::Drying, Lang::En) => "Drying the air",
            (Self::Drying, Lang::De) => "Trocknet die Luft",
            (Self::Drying, Lang::Tr) => "Havayı kurutuyor",
            (Self::Fan, Lang::En) => "Moving the air",
            (Self::Fan, Lang::De) => "Bewegt die Luft",
            (Self::Fan, Lang::Tr) => "Havayı hareket ettiriyor",
            (Self::Preheating, Lang::En) => "Warming up",
            (Self::Preheating, Lang::De) => "Wärmt sich auf",
            (Self::Preheating, Lang::Tr) => "Isınıyor",
            (Self::Defrosting, Lang::En) => "Clearing frost",
            (Self::Defrosting, Lang::De) => "Enteist",
            (Self::Defrosting, Lang::Tr) => "Buz çözülüyor",
        }
    }
}

impl FanMode {
    /// Plain-language fan-speed name.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Auto, Lang::En) => "Automatic",
            (Self::Auto, Lang::De) => "Automatisch",
            (Self::Auto, Lang::Tr) => "Otomatik",
            (Self::On, Lang::En) => "Always on",
            (Self::On, Lang::De) => "Immer an",
            (Self::On, Lang::Tr) => "Her zaman açık",
            (Self::Low, Lang::En) => "Low",
            (Self::Low, Lang::De) => "Niedrig",
            (Self::Low, Lang::Tr) => "Düşük",
            (Self::Medium, Lang::En) => "Medium",
            (Self::Medium, Lang::De) => "Mittel",
            (Self::Medium, Lang::Tr) => "Orta",
            (Self::High, Lang::En) => "High",
            (Self::High, Lang::De) => "Hoch",
            (Self::High, Lang::Tr) => "Yüksek",
        }
    }
}

impl PresetMode {
    /// Plain-language preset name.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::None, Lang::En) => "Normal",
            (Self::None, Lang::De) => "Normal",
            (Self::None, Lang::Tr) => "Normal",
            (Self::Eco, Lang::En) => "Energy saving",
            (Self::Eco, Lang::De) => "Energiesparen",
            (Self::Eco, Lang::Tr) => "Enerji tasarrufu",
            (Self::Away, Lang::En) => "Away",
            (Self::Away, Lang::De) => "Abwesend",
            (Self::Away, Lang::Tr) => "Dışarıda",
            (Self::Boost, Lang::En) => "Quick boost",
            (Self::Boost, Lang::De) => "Schnellschub",
            (Self::Boost, Lang::Tr) => "Hızlı güçlendirme",
            (Self::Comfort, Lang::En) => "Comfort",
            (Self::Comfort, Lang::De) => "Komfort",
            (Self::Comfort, Lang::Tr) => "Konfor",
            (Self::Home, Lang::En) => "At home",
            (Self::Home, Lang::De) => "Zu Hause",
            (Self::Home, Lang::Tr) => "Evde",
            (Self::Sleep, Lang::En) => "Sleep",
            (Self::Sleep, Lang::De) => "Schlafen",
            (Self::Sleep, Lang::Tr) => "Uyku",
            (Self::Activity, Lang::En) => "Active",
            (Self::Activity, Lang::De) => "Aktiv",
            (Self::Activity, Lang::Tr) => "Aktif",
        }
    }
}

/// A one-line, household-level sentence describing the current action, with the
/// target temperature woven in where it helps (e.g. "Heating to 21°").
///
/// `target_celsius` is the relevant setpoint for the active mode; pass `None`
/// for modes (Off, `FanOnly`) where a target is meaningless.
#[must_use]
pub fn action_sentence(action: HvacAction, target_celsius: Option<f64>, lang: Lang) -> String {
    match (action, target_celsius) {
        (HvacAction::Heating, Some(t)) => match lang {
            Lang::En => format!("Heating to {}°", round_half(t)),
            Lang::De => format!("Heizt auf {}°", round_half(t)),
            Lang::Tr => format!("{}° dereceye ısıtıyor", round_half(t)),
        },
        (HvacAction::Cooling, Some(t)) => match lang {
            Lang::En => format!("Cooling to {}°", round_half(t)),
            Lang::De => format!("Kühlt auf {}°", round_half(t)),
            Lang::Tr => format!("{}° dereceye soğutuyor", round_half(t)),
        },
        // For every other action (or a missing target) the plain action label is
        // the whole sentence.
        (a, _) => a.label(lang).to_string(),
    }
}

/// Round to the nearest half-degree for display, trimming a trailing `.0`.
fn round_half(celsius: f64) -> String {
    let rounded = (celsius * 2.0).round() / 2.0;
    if (rounded - rounded.trunc()).abs() < 1e-9 {
        format!("{}", rounded.trunc() as i64)
    } else {
        format!("{rounded}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [Lang; 3] = [Lang::En, Lang::De, Lang::Tr];

    #[test]
    fn every_mode_has_three_language_labels() {
        for m in HvacMode::ALL {
            for lang in LANGS {
                assert!(!m.label(lang).is_empty(), "{m:?} missing label");
            }
        }
    }

    #[test]
    fn every_action_has_three_language_labels() {
        for a in [
            HvacAction::Off,
            HvacAction::Idle,
            HvacAction::Heating,
            HvacAction::Cooling,
            HvacAction::Drying,
            HvacAction::Fan,
            HvacAction::Preheating,
            HvacAction::Defrosting,
        ] {
            for lang in LANGS {
                assert!(!a.label(lang).is_empty(), "{a:?} missing label");
            }
        }
    }

    #[test]
    fn every_fan_and_preset_has_three_language_labels() {
        for f in FanMode::ALL {
            for lang in LANGS {
                assert!(!f.label(lang).is_empty(), "{f:?} missing label");
            }
        }
        for p in PresetMode::ALL {
            for lang in LANGS {
                assert!(!p.label(lang).is_empty(), "{p:?} missing label");
            }
        }
    }

    #[test]
    fn heating_sentence_weaves_in_the_target() {
        assert_eq!(
            action_sentence(HvacAction::Heating, Some(21.0), Lang::En),
            "Heating to 21°"
        );
        assert_eq!(
            action_sentence(HvacAction::Cooling, Some(24.5), Lang::En),
            "Cooling to 24.5°"
        );
    }

    #[test]
    fn idle_sentence_is_the_reassuring_label() {
        assert_eq!(
            action_sentence(HvacAction::Idle, Some(21.0), Lang::En),
            "Reached your temperature"
        );
    }

    #[test]
    fn round_half_trims_whole_degrees() {
        assert_eq!(round_half(21.0), "21");
        assert_eq!(round_half(21.5), "21.5");
        assert_eq!(round_half(21.24), "21");
        assert_eq!(round_half(21.26), "21.5");
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol / vendor / API terms.
        const BANNED: &[&str] = &[
            "HVAC",
            "hvac_action",
            "HeatCool",
            "FanOnly",
            "Modbus",
            "register",
            "MQTT",
            "Zigbee",
            "Z-Wave",
            "Matter",
            "entity_id",
            "Open3EClient",
            "ViCare",
            "endpoint",
            "API",
            "setpoint",
            "hysteresis",
            "tolerance",
        ];
        let mut texts: Vec<String> = Vec::new();
        for lang in LANGS {
            for m in HvacMode::ALL {
                texts.push(m.label(lang).to_string());
            }
            for f in FanMode::ALL {
                texts.push(f.label(lang).to_string());
            }
            for p in PresetMode::ALL {
                texts.push(p.label(lang).to_string());
            }
            for a in [
                HvacAction::Off,
                HvacAction::Idle,
                HvacAction::Heating,
                HvacAction::Cooling,
                HvacAction::Drying,
                HvacAction::Fan,
                HvacAction::Preheating,
                HvacAction::Defrosting,
            ] {
                texts.push(a.label(lang).to_string());
                texts.push(action_sentence(a, Some(21.0), lang));
            }
        }
        for text in texts {
            for banned in BANNED {
                assert!(
                    !text.contains(banned),
                    "user-facing string leaks jargon {banned:?}: {text:?}"
                );
            }
        }
    }
}
