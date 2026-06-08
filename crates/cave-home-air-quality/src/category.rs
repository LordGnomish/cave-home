//! Grandma-friendly air-quality categories (Charter §6.3, ADR-007).
//!
//! The numeric AQI never reaches the end-user. The Portal and mobile app show
//! a [`AirCategory`] — a six-step band with a plain-language name, a traffic-
//! light colour, and a recommended action — localised to EN / DE / TR (the
//! Charter §6.3 mandatory languages from M1).

/// The six air-quality bands, ordered best → worst. Mirrors the EPA AQI
/// category boundaries but is named in household language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AirCategory {
    /// AQI 0–50.
    Good,
    /// AQI 51–100.
    Fair,
    /// AQI 101–150 (EPA "Unhealthy for Sensitive Groups").
    Sensitive,
    /// AQI 151–200.
    Poor,
    /// AQI 201–300.
    VeryPoor,
    /// AQI 301+.
    Hazardous,
}

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl AirCategory {
    /// Map a numeric AQI to its band.
    #[must_use]
    pub const fn from_aqi(aqi: u16) -> Self {
        match aqi {
            0..=50 => Self::Good,
            51..=100 => Self::Fair,
            101..=150 => Self::Sensitive,
            151..=200 => Self::Poor,
            201..=300 => Self::VeryPoor,
            _ => Self::Hazardous,
        }
    }

    /// Traffic-light colour as a hex string for the dashboard tile.
    #[must_use]
    pub const fn color_hex(self) -> &'static str {
        match self {
            Self::Good => "#00e400",
            Self::Fair => "#ffff00",
            Self::Sensitive => "#ff7e00",
            Self::Poor => "#ff0000",
            Self::VeryPoor => "#8f3f97",
            Self::Hazardous => "#7e0023",
        }
    }

    /// Localised band name (no jargon — Charter §6.3).
    #[must_use]
    pub const fn name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Good, Lang::En) => "Good",
            (Self::Good, Lang::De) => "Gut",
            (Self::Good, Lang::Tr) => "İyi",
            (Self::Fair, Lang::En) => "Fair",
            (Self::Fair, Lang::De) => "Mäßig",
            (Self::Fair, Lang::Tr) => "Orta",
            (Self::Sensitive, Lang::En) => "A bit much for sensitive people",
            (Self::Sensitive, Lang::De) => "Etwas viel für empfindliche Personen",
            (Self::Sensitive, Lang::Tr) => "Hassas kişiler için biraz fazla",
            (Self::Poor, Lang::En) => "Poor",
            (Self::Poor, Lang::De) => "Schlecht",
            (Self::Poor, Lang::Tr) => "Kötü",
            (Self::VeryPoor, Lang::En) => "Very poor",
            (Self::VeryPoor, Lang::De) => "Sehr schlecht",
            (Self::VeryPoor, Lang::Tr) => "Çok kötü",
            (Self::Hazardous, Lang::En) => "Dangerous",
            (Self::Hazardous, Lang::De) => "Gefährlich",
            (Self::Hazardous, Lang::Tr) => "Tehlikeli",
        }
    }

    /// A concrete, household-level recommended action.
    #[must_use]
    pub const fn advice(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Good, Lang::En) => "Air is clean — nothing to do.",
            (Self::Good, Lang::De) => "Die Luft ist sauber — nichts zu tun.",
            (Self::Good, Lang::Tr) => "Hava temiz — yapılacak bir şey yok.",
            (Self::Fair, Lang::En) => "Air is okay.",
            (Self::Fair, Lang::De) => "Die Luft ist in Ordnung.",
            (Self::Fair, Lang::Tr) => "Hava idare eder.",
            (Self::Sensitive, Lang::En) => "Open a window if anyone is sensitive.",
            (Self::Sensitive, Lang::De) => "Bei empfindlichen Personen ein Fenster öffnen.",
            (Self::Sensitive, Lang::Tr) => "Hassas biri varsa pencere açın.",
            (Self::Poor, Lang::En) => "Open a window to let in fresh air.",
            (Self::Poor, Lang::De) => "Ein Fenster öffnen, um frische Luft hereinzulassen.",
            (Self::Poor, Lang::Tr) => "Temiz hava için pencere açın.",
            (Self::VeryPoor, Lang::En) => "Ventilate now and avoid strenuous activity indoors.",
            (Self::VeryPoor, Lang::De) => "Jetzt lüften und Anstrengung drinnen vermeiden.",
            (Self::VeryPoor, Lang::Tr) => "Hemen havalandırın, içeride yorucu işten kaçının.",
            (Self::Hazardous, Lang::En) => "Ventilate immediately and check for a source.",
            (Self::Hazardous, Lang::De) => "Sofort lüften und nach der Ursache suchen.",
            (Self::Hazardous, Lang::Tr) => "Hemen havalandırın ve kaynağı kontrol edin.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aqi_boundaries_map_to_bands() {
        assert_eq!(AirCategory::from_aqi(0), AirCategory::Good);
        assert_eq!(AirCategory::from_aqi(50), AirCategory::Good);
        assert_eq!(AirCategory::from_aqi(51), AirCategory::Fair);
        assert_eq!(AirCategory::from_aqi(100), AirCategory::Fair);
        assert_eq!(AirCategory::from_aqi(101), AirCategory::Sensitive);
        assert_eq!(AirCategory::from_aqi(150), AirCategory::Sensitive);
        assert_eq!(AirCategory::from_aqi(151), AirCategory::Poor);
        assert_eq!(AirCategory::from_aqi(200), AirCategory::Poor);
        assert_eq!(AirCategory::from_aqi(201), AirCategory::VeryPoor);
        assert_eq!(AirCategory::from_aqi(300), AirCategory::VeryPoor);
        assert_eq!(AirCategory::from_aqi(301), AirCategory::Hazardous);
        assert_eq!(AirCategory::from_aqi(500), AirCategory::Hazardous);
    }

    #[test]
    fn ordering_is_best_to_worst() {
        assert!(AirCategory::Good < AirCategory::Fair);
        assert!(AirCategory::Poor < AirCategory::Hazardous);
        assert_eq!(
            AirCategory::Good.max(AirCategory::Poor),
            AirCategory::Poor,
            "worst-of aggregation picks the higher (worse) band"
        );
    }

    #[test]
    fn all_bands_have_three_language_names_and_advice() {
        for cat in [
            AirCategory::Good,
            AirCategory::Fair,
            AirCategory::Sensitive,
            AirCategory::Poor,
            AirCategory::VeryPoor,
            AirCategory::Hazardous,
        ] {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!cat.name(lang).is_empty());
                assert!(!cat.advice(lang).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol/cluster terms.
        const BANNED: &[&str] = &[
            "AQI", "PM2.5", "PM10", "ppm", "ppb", "MQTT", "Zigbee", "µg",
            "pod", "kubelet",
        ];
        for cat in [
            AirCategory::Good,
            AirCategory::Fair,
            AirCategory::Sensitive,
            AirCategory::Poor,
            AirCategory::VeryPoor,
            AirCategory::Hazardous,
        ] {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let text = format!("{} {}", cat.name(lang), cat.advice(lang));
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "band {cat:?} leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
