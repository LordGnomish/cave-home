//! Grandma-friendly wellness bands (Charter §6.3, ADR-025, ADR-007).
//!
//! Each metric maps to a small, plain-language band with a gentle, encouraging
//! piece of advice in EN / DE / TR. The thresholds come from **public,
//! general-population wellness guidance** (the kind printed on a fitness-app
//! home screen), not from clinical diagnostic criteria.
//!
//! These are **wellness signals, not medical advice or diagnosis.** The copy is
//! intentionally warm and free of clinical / alarming words — "you slept well",
//! "nice walk", never "arrhythmia", "hypertension", or "BMI percentile". A user
//! who wants a medical opinion should see a clinician; cave-home only nudges.

use crate::label::Lang;

/// Resting-heart-rate band, ordered calm → elevated.
///
/// Thresholds follow common adult resting-heart-rate reference ranges (the
/// general "normal adult resting pulse is roughly 60–100 bpm" guidance), with a
/// "Low" band for the well-trained-resting / bradycardic-leaning range and an
/// "Elevated" step before "High". Wellness framing only — not a diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RestingHrBand {
    /// Below the usual adult resting range (common in fit, rested people).
    Low,
    /// The usual adult resting range.
    Normal,
    /// A little above the usual resting range.
    Elevated,
    /// Well above the usual resting range.
    High,
}

impl RestingHrBand {
    /// Classify a resting heart rate in bpm into a wellness band.
    #[must_use]
    pub const fn from_bpm(bpm: u16) -> Self {
        match bpm {
            0..=59 => Self::Low,
            60..=89 => Self::Normal,
            90..=99 => Self::Elevated,
            _ => Self::High,
        }
    }

    /// Localized, non-clinical band name.
    #[must_use]
    pub const fn name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Low, Lang::En) => "Nice and calm",
            (Self::Low, Lang::De) => "Schön ruhig",
            (Self::Low, Lang::Tr) => "Güzel ve sakin",
            (Self::Normal, Lang::En) => "Steady",
            (Self::Normal, Lang::De) => "Gleichmäßig",
            (Self::Normal, Lang::Tr) => "Dengeli",
            (Self::Elevated, Lang::En) => "A little high",
            (Self::Elevated, Lang::De) => "Etwas erhöht",
            (Self::Elevated, Lang::Tr) => "Biraz yüksek",
            (Self::High, Lang::En) => "Higher than usual",
            (Self::High, Lang::De) => "Höher als sonst",
            (Self::High, Lang::Tr) => "Her zamankinden yüksek",
        }
    }

    /// Localized, gentle advice. Never clinical or alarming.
    #[must_use]
    pub const fn advice(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Low, Lang::En) => "Your resting pulse looks relaxed.",
            (Self::Low, Lang::De) => "Dein Ruhepuls wirkt entspannt.",
            (Self::Low, Lang::Tr) => "Dinlenme nabzın rahat görünüyor.",
            (Self::Normal, Lang::En) => "All looks steady.",
            (Self::Normal, Lang::De) => "Alles wirkt gleichmäßig.",
            (Self::Normal, Lang::Tr) => "Her şey dengeli görünüyor.",
            (Self::Elevated, Lang::En) => "Maybe take a calm moment and some water.",
            (Self::Elevated, Lang::De) => "Vielleicht eine ruhige Pause und etwas Wasser.",
            (Self::Elevated, Lang::Tr) => "Belki sakin bir mola ve biraz su iyi gelir.",
            (Self::High, Lang::En) => "Rest a little; check in with someone you trust if it stays up.",
            (Self::High, Lang::De) => "Ruh dich etwas aus; sprich mit jemandem, dem du vertraust, wenn es so hoch ist.",
            (Self::High, Lang::Tr) => "Biraz dinlen; böyle devam ederse güvendiğin biriyle konuş.",
        }
    }
}

/// Sleep-duration band, ordered too-little → too-much.
///
/// Thresholds follow public sleep-foundation general-adult guidance of roughly
/// 7–9 hours a night: under 7h is "Insufficient", 7h up to 9h is "Adequate",
/// the 8–9h sweet spot is "Optimal", and over 9h is "Excessive". Wellness
/// framing only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SleepBand {
    /// Under the general adult guidance.
    Insufficient,
    /// Within the general adult guidance.
    Adequate,
    /// Within the recommended sweet spot.
    Optimal,
    /// Above the general adult guidance.
    Excessive,
}

impl SleepBand {
    /// Classify a sleep duration (in minutes) into a wellness band.
    ///
    /// Boundaries: `< 420` Insufficient, `420..=479` Adequate,
    /// `480..=540` Optimal, `> 540` Excessive (7h / 8h / 9h in minutes).
    #[must_use]
    pub const fn from_minutes(minutes: u16) -> Self {
        match minutes {
            0..=419 => Self::Insufficient,
            420..=479 => Self::Adequate,
            480..=540 => Self::Optimal,
            _ => Self::Excessive,
        }
    }

    /// Localized, non-clinical band name.
    #[must_use]
    pub const fn name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Insufficient, Lang::En) => "A short night",
            (Self::Insufficient, Lang::De) => "Eine kurze Nacht",
            (Self::Insufficient, Lang::Tr) => "Kısa bir gece",
            (Self::Adequate, Lang::En) => "A good night",
            (Self::Adequate, Lang::De) => "Eine gute Nacht",
            (Self::Adequate, Lang::Tr) => "İyi bir gece",
            (Self::Optimal, Lang::En) => "A great night",
            (Self::Optimal, Lang::De) => "Eine tolle Nacht",
            (Self::Optimal, Lang::Tr) => "Harika bir gece",
            (Self::Excessive, Lang::En) => "A long sleep",
            (Self::Excessive, Lang::De) => "Ein langer Schlaf",
            (Self::Excessive, Lang::Tr) => "Uzun bir uyku",
        }
    }

    /// Localized, gentle advice. Never clinical or alarming.
    #[must_use]
    pub const fn advice(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Insufficient, Lang::En) => "Try to wind down a little earlier tonight.",
            (Self::Insufficient, Lang::De) => "Versuch heute, etwas früher zur Ruhe zu kommen.",
            (Self::Insufficient, Lang::Tr) => "Bu gece biraz daha erken dinlenmeye çalış.",
            (Self::Adequate, Lang::En) => "You slept well last night.",
            (Self::Adequate, Lang::De) => "Du hast letzte Nacht gut geschlafen.",
            (Self::Adequate, Lang::Tr) => "Dün gece iyi uyudun.",
            (Self::Optimal, Lang::En) => "Lovely rest — keep it up.",
            (Self::Optimal, Lang::De) => "Schöne Erholung — weiter so.",
            (Self::Optimal, Lang::Tr) => "Güzel bir dinlenme — böyle devam.",
            (Self::Excessive, Lang::En) => "A nice long rest; a little daylight can help you feel fresh.",
            (Self::Excessive, Lang::De) => "Eine schöne lange Erholung; etwas Tageslicht macht munter.",
            (Self::Excessive, Lang::Tr) => "Güzel uzun bir dinlenme; biraz gün ışığı zinde hissettirir.",
        }
    }
}

/// Step-activity band, ordered least → most active.
///
/// Thresholds follow widely-published daily-step activity tiers (the common
/// "under 5,000 sedentary; 5,000–7,499 low; 7,500–9,999 active; 10,000+ very
/// active" framing). Wellness framing only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActivityBand {
    /// A quiet day on your feet.
    Sedentary,
    /// A little movement.
    Low,
    /// A nicely active day.
    Active,
    /// A very active day.
    VeryActive,
}

impl ActivityBand {
    /// Classify a daily step count into a wellness band.
    ///
    /// Boundaries: `< 5000` Sedentary, `5000..=7499` Low,
    /// `7500..=9999` Active, `10000` and up `VeryActive`.
    #[must_use]
    pub const fn from_steps(steps: u32) -> Self {
        match steps {
            0..=4_999 => Self::Sedentary,
            5_000..=7_499 => Self::Low,
            7_500..=9_999 => Self::Active,
            _ => Self::VeryActive,
        }
    }

    /// Localized, non-clinical band name.
    #[must_use]
    pub const fn name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Sedentary, Lang::En) => "A quiet day",
            (Self::Sedentary, Lang::De) => "Ein ruhiger Tag",
            (Self::Sedentary, Lang::Tr) => "Sakin bir gün",
            (Self::Low, Lang::En) => "A little walking",
            (Self::Low, Lang::De) => "Etwas Bewegung",
            (Self::Low, Lang::Tr) => "Biraz yürüyüş",
            (Self::Active, Lang::En) => "A nice active day",
            (Self::Active, Lang::De) => "Ein schön aktiver Tag",
            (Self::Active, Lang::Tr) => "Güzel hareketli bir gün",
            (Self::VeryActive, Lang::En) => "A very active day",
            (Self::VeryActive, Lang::De) => "Ein sehr aktiver Tag",
            (Self::VeryActive, Lang::Tr) => "Çok hareketli bir gün",
        }
    }

    /// Localized, gentle advice. Never clinical or alarming.
    #[must_use]
    pub const fn advice(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Sedentary, Lang::En) => "Try a short walk when you can.",
            (Self::Sedentary, Lang::De) => "Mach einen kurzen Spaziergang, wenn du kannst.",
            (Self::Sedentary, Lang::Tr) => "Fırsat bulunca kısa bir yürüyüş yap.",
            (Self::Low, Lang::En) => "Nice start — a little more would feel good.",
            (Self::Low, Lang::De) => "Schöner Anfang — etwas mehr täte gut.",
            (Self::Low, Lang::Tr) => "Güzel başlangıç — biraz daha iyi gelir.",
            (Self::Active, Lang::En) => "Nice walking today.",
            (Self::Active, Lang::De) => "Schön gelaufen heute.",
            (Self::Active, Lang::Tr) => "Bugün güzel yürüdün.",
            (Self::VeryActive, Lang::En) => "Wonderful — you moved a lot today.",
            (Self::VeryActive, Lang::De) => "Wunderbar — du hast dich heute viel bewegt.",
            (Self::VeryActive, Lang::Tr) => "Harika — bugün çok hareket ettin.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resting_hr_band_boundaries() {
        assert_eq!(RestingHrBand::from_bpm(50), RestingHrBand::Low);
        assert_eq!(RestingHrBand::from_bpm(59), RestingHrBand::Low);
        assert_eq!(RestingHrBand::from_bpm(60), RestingHrBand::Normal);
        assert_eq!(RestingHrBand::from_bpm(89), RestingHrBand::Normal);
        assert_eq!(RestingHrBand::from_bpm(90), RestingHrBand::Elevated);
        assert_eq!(RestingHrBand::from_bpm(99), RestingHrBand::Elevated);
        assert_eq!(RestingHrBand::from_bpm(100), RestingHrBand::High);
        assert_eq!(RestingHrBand::from_bpm(140), RestingHrBand::High);
    }

    #[test]
    fn sleep_band_boundaries() {
        assert_eq!(SleepBand::from_minutes(0), SleepBand::Insufficient);
        assert_eq!(SleepBand::from_minutes(419), SleepBand::Insufficient);
        assert_eq!(SleepBand::from_minutes(420), SleepBand::Adequate); // 7h
        assert_eq!(SleepBand::from_minutes(479), SleepBand::Adequate);
        assert_eq!(SleepBand::from_minutes(480), SleepBand::Optimal); // 8h
        assert_eq!(SleepBand::from_minutes(540), SleepBand::Optimal); // 9h
        assert_eq!(SleepBand::from_minutes(541), SleepBand::Excessive);
    }

    #[test]
    fn activity_band_boundaries() {
        assert_eq!(ActivityBand::from_steps(0), ActivityBand::Sedentary);
        assert_eq!(ActivityBand::from_steps(4_999), ActivityBand::Sedentary);
        assert_eq!(ActivityBand::from_steps(5_000), ActivityBand::Low);
        assert_eq!(ActivityBand::from_steps(7_499), ActivityBand::Low);
        assert_eq!(ActivityBand::from_steps(7_500), ActivityBand::Active);
        assert_eq!(ActivityBand::from_steps(9_999), ActivityBand::Active);
        assert_eq!(ActivityBand::from_steps(10_000), ActivityBand::VeryActive);
    }

    #[test]
    fn band_ordering_is_meaningful() {
        assert!(SleepBand::Insufficient < SleepBand::Adequate);
        assert!(ActivityBand::Sedentary < ActivityBand::VeryActive);
        assert!(RestingHrBand::Normal < RestingHrBand::High);
    }

    #[test]
    fn all_bands_have_three_language_names_and_advice() {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            for b in [
                RestingHrBand::Low,
                RestingHrBand::Normal,
                RestingHrBand::Elevated,
                RestingHrBand::High,
            ] {
                assert!(!b.name(lang).is_empty());
                assert!(!b.advice(lang).is_empty());
            }
            for b in [
                SleepBand::Insufficient,
                SleepBand::Adequate,
                SleepBand::Optimal,
                SleepBand::Excessive,
            ] {
                assert!(!b.name(lang).is_empty());
                assert!(!b.advice(lang).is_empty());
            }
            for b in [
                ActivityBand::Sedentary,
                ActivityBand::Low,
                ActivityBand::Active,
                ActivityBand::VeryActive,
            ] {
                assert!(!b.name(lang).is_empty());
                assert!(!b.advice(lang).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol / implementation terms.
        const BANNED: &[&str] = &[
            "MQTT", "Zigbee", "OAuth", "API", "token", "endpoint", "entity_id",
            "pod", "kubelet", "BLE", "Fitbit", "Withings", "Garmin", "bpm",
        ];
        assert_no_terms(BANNED, "jargon");
    }

    #[test]
    fn ui_strings_carry_no_clinical_diagnosis_words() {
        // ADR-025 / Charter §6.3: wellness copy must stay gentle and non-clinical.
        const CLINICAL: &[&str] = &[
            "arrhythmia", "hypertension", "hypotension", "tachycardia",
            "bradycardia", "diagnosis", "disease", "disorder", "BMI",
            "percentile", "abnormal", "symptom", "syndrome", "obese",
            "obesity", "apnea", "insomnia", "clinical", "medical",
        ];
        assert_no_terms(CLINICAL, "clinical");
    }

    fn assert_no_terms(terms: &[&str], kind: &str) {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            let mut texts: Vec<String> = Vec::new();
            for b in [
                RestingHrBand::Low,
                RestingHrBand::Normal,
                RestingHrBand::Elevated,
                RestingHrBand::High,
            ] {
                texts.push(format!("{} {}", b.name(lang), b.advice(lang)));
            }
            for b in [
                SleepBand::Insufficient,
                SleepBand::Adequate,
                SleepBand::Optimal,
                SleepBand::Excessive,
            ] {
                texts.push(format!("{} {}", b.name(lang), b.advice(lang)));
            }
            for b in [
                ActivityBand::Sedentary,
                ActivityBand::Low,
                ActivityBand::Active,
                ActivityBand::VeryActive,
            ] {
                texts.push(format!("{} {}", b.name(lang), b.advice(lang)));
            }
            for text in texts {
                let lower = text.to_lowercase();
                for term in terms {
                    assert!(
                        !lower.contains(&term.to_lowercase()),
                        "copy leaks {kind} term {term:?}: {text}"
                    );
                }
            }
        }
    }
}
