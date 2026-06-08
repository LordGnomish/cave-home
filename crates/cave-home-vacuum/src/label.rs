//! Grandma-friendly labels for vacuum states and faults (Charter §6.3,
//! ADR-007).
//!
//! The end-user never sees `Returning`, an error code or a vendor term — they
//! see "Vacuum is cleaning", "Vacuum is heading back to its dock", or "Vacuum is
//! stuck — please free its brush", localised to EN / DE / TR (the Charter §6.3
//! languages mandatory from M1). This module is the only place vacuum state and
//! faults become words a household reads.

use crate::error::ErrorCode;
use crate::state::VacuumState;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl VacuumState {
    /// A short, plain-language status line for this state — what the household
    /// sees on the vacuum tile. No vendor, protocol or implementation words.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Idle, Lang::En) => "Vacuum is waiting",
            (Self::Idle, Lang::De) => "Sauger wartet",
            (Self::Idle, Lang::Tr) => "Süpürge bekliyor",
            (Self::Cleaning, Lang::En) => "Vacuum is cleaning",
            (Self::Cleaning, Lang::De) => "Sauger reinigt gerade",
            (Self::Cleaning, Lang::Tr) => "Süpürge temizlik yapıyor",
            (Self::SpotCleaning, Lang::En) => "Vacuum is cleaning one spot",
            (Self::SpotCleaning, Lang::De) => "Sauger reinigt eine Stelle",
            (Self::SpotCleaning, Lang::Tr) => "Süpürge tek bir noktayı temizliyor",
            (Self::Returning, Lang::En) => "Vacuum is heading back to its dock",
            (Self::Returning, Lang::De) => "Sauger fährt zur Ladestation zurück",
            (Self::Returning, Lang::Tr) => "Süpürge şarj istasyonuna dönüyor",
            (Self::Docked, Lang::En) => "Vacuum is on its dock",
            (Self::Docked, Lang::De) => "Sauger steht auf der Ladestation",
            (Self::Docked, Lang::Tr) => "Süpürge şarj istasyonunda",
            (Self::Paused, Lang::En) => "Vacuum is paused",
            (Self::Paused, Lang::De) => "Sauger pausiert",
            (Self::Paused, Lang::Tr) => "Süpürge duraklatıldı",
            (Self::Error, Lang::En) => "Vacuum needs help",
            (Self::Error, Lang::De) => "Sauger braucht Hilfe",
            (Self::Error, Lang::Tr) => "Süpürgenin yardıma ihtiyacı var",
            (Self::Manual, Lang::En) => "You are steering the vacuum",
            (Self::Manual, Lang::De) => "Du steuerst den Sauger",
            (Self::Manual, Lang::Tr) => "Süpürgeyi siz yönlendiriyorsunuz",
        }
    }

    /// A concrete, household-level note about what to do (or that nothing is
    /// needed) for this state.
    #[must_use]
    pub const fn advice(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Idle, Lang::En) => "Tap clean when you are ready.",
            (Self::Idle, Lang::De) => "Auf Reinigen tippen, wenn du bereit bist.",
            (Self::Idle, Lang::Tr) => "Hazır olunca temizliğe başlatın.",
            (Self::Cleaning | Self::SpotCleaning, Lang::En) => "All good — let it work.",
            (Self::Cleaning | Self::SpotCleaning, Lang::De) => "Alles gut — lass es arbeiten.",
            (Self::Cleaning | Self::SpotCleaning, Lang::Tr) => "Her şey yolunda — bırakın çalışsın.",
            (Self::Returning, Lang::En) => "Nothing to do — it is going home.",
            (Self::Returning, Lang::De) => "Nichts zu tun — es fährt nach Hause.",
            (Self::Returning, Lang::Tr) => "Yapacak bir şey yok — yerine dönüyor.",
            (Self::Docked, Lang::En) => "Resting and charging.",
            (Self::Docked, Lang::De) => "Ruht und lädt.",
            (Self::Docked, Lang::Tr) => "Dinleniyor ve şarj oluyor.",
            (Self::Paused, Lang::En) => "Tap clean to carry on.",
            (Self::Paused, Lang::De) => "Auf Reinigen tippen, um fortzufahren.",
            (Self::Paused, Lang::Tr) => "Devam etmek için temizliğe dokunun.",
            (Self::Error, Lang::En) => "Please check on the vacuum.",
            (Self::Error, Lang::De) => "Bitte nach dem Sauger sehen.",
            (Self::Error, Lang::Tr) => "Lütfen süpürgeyi kontrol edin.",
            (Self::Manual, Lang::En) => "Use the arrows to drive it.",
            (Self::Manual, Lang::De) => "Mit den Pfeilen steuern.",
            (Self::Manual, Lang::Tr) => "Yönlendirmek için okları kullanın.",
        }
    }
}

impl ErrorCode {
    /// A plain-language description of what is wrong — what the household reads
    /// when the vacuum needs help. No code, no vendor term.
    #[must_use]
    pub const fn explanation(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::BrushStuck, Lang::En) => "The brush is stuck.",
            (Self::BrushStuck, Lang::De) => "Die Bürste steckt fest.",
            (Self::BrushStuck, Lang::Tr) => "Fırça sıkıştı.",
            (Self::WheelStuck, Lang::En) => "A wheel is stuck.",
            (Self::WheelStuck, Lang::De) => "Ein Rad steckt fest.",
            (Self::WheelStuck, Lang::Tr) => "Bir tekerlek sıkıştı.",
            (Self::SideBrushStuck, Lang::En) => "The side brush is tangled.",
            (Self::SideBrushStuck, Lang::De) => "Die Seitenbürste ist verheddert.",
            (Self::SideBrushStuck, Lang::Tr) => "Yan fırça dolaşmış.",
            (Self::BinFull, Lang::En) => "The dust container is full.",
            (Self::BinFull, Lang::De) => "Der Staubbehälter ist voll.",
            (Self::BinFull, Lang::Tr) => "Toz haznesi dolu.",
            (Self::DustbinMissing, Lang::En) => "The dust container is not in place.",
            (Self::DustbinMissing, Lang::De) => "Der Staubbehälter fehlt.",
            (Self::DustbinMissing, Lang::Tr) => "Toz haznesi yerinde değil.",
            (Self::Lost, Lang::En) => "The vacuum cannot find where it is.",
            (Self::Lost, Lang::De) => "Der Sauger findet sich nicht zurecht.",
            (Self::Lost, Lang::Tr) => "Süpürge nerede olduğunu bulamıyor.",
            (Self::Trapped, Lang::En) => "The vacuum is stuck and cannot move.",
            (Self::Trapped, Lang::De) => "Der Sauger sitzt fest und kommt nicht weiter.",
            (Self::Trapped, Lang::Tr) => "Süpürge sıkışmış ve hareket edemiyor.",
            (Self::CliffSensor, Lang::En) => "The vacuum stopped at a drop.",
            (Self::CliffSensor, Lang::De) => "Der Sauger hat an einer Kante gestoppt.",
            (Self::CliffSensor, Lang::Tr) => "Süpürge bir kenarda durdu.",
            (Self::WaterTankEmpty, Lang::En) => "The water tank is empty.",
            (Self::WaterTankEmpty, Lang::De) => "Der Wassertank ist leer.",
            (Self::WaterTankEmpty, Lang::Tr) => "Su deposu boş.",
            (Self::Generic, Lang::En) => "The vacuum ran into a problem.",
            (Self::Generic, Lang::De) => "Der Sauger hat ein Problem.",
            (Self::Generic, Lang::Tr) => "Süpürge bir sorunla karşılaştı.",
        }
    }

    /// A concrete, household-level recommended action for this fault.
    #[must_use]
    pub const fn advice(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::BrushStuck, Lang::En) => "Please free its brush.",
            (Self::BrushStuck, Lang::De) => "Bitte die Bürste frei machen.",
            (Self::BrushStuck, Lang::Tr) => "Lütfen fırçasını kurtarın.",
            (Self::WheelStuck, Lang::En) => "Lift it and clear the wheel.",
            (Self::WheelStuck, Lang::De) => "Anheben und das Rad frei machen.",
            (Self::WheelStuck, Lang::Tr) => "Kaldırıp tekerleği temizleyin.",
            (Self::SideBrushStuck, Lang::En) => "Please untangle the side brush.",
            (Self::SideBrushStuck, Lang::De) => "Bitte die Seitenbürste freimachen.",
            (Self::SideBrushStuck, Lang::Tr) => "Lütfen yan fırçayı çözün.",
            (Self::BinFull, Lang::En) => "Please empty the dust container.",
            (Self::BinFull, Lang::De) => "Bitte den Staubbehälter leeren.",
            (Self::BinFull, Lang::Tr) => "Lütfen toz haznesini boşaltın.",
            (Self::DustbinMissing, Lang::En) => "Please put the dust container back.",
            (Self::DustbinMissing, Lang::De) => "Bitte den Staubbehälter wieder einsetzen.",
            (Self::DustbinMissing, Lang::Tr) => "Lütfen toz haznesini geri takın.",
            (Self::Lost, Lang::En) => "Carry it back near its dock.",
            (Self::Lost, Lang::De) => "Bitte zurück in die Nähe der Ladestation stellen.",
            (Self::Lost, Lang::Tr) => "Şarj istasyonunun yanına geri getirin.",
            (Self::Trapped, Lang::En) => "Please free it and set it down.",
            (Self::Trapped, Lang::De) => "Bitte befreien und absetzen.",
            (Self::Trapped, Lang::Tr) => "Lütfen kurtarıp yere bırakın.",
            (Self::CliffSensor, Lang::En) => "Move it away from the edge.",
            (Self::CliffSensor, Lang::De) => "Von der Kante wegstellen.",
            (Self::CliffSensor, Lang::Tr) => "Kenardan uzaklaştırın.",
            (Self::WaterTankEmpty, Lang::En) => "Please refill the water tank.",
            (Self::WaterTankEmpty, Lang::De) => "Bitte den Wassertank auffüllen.",
            (Self::WaterTankEmpty, Lang::Tr) => "Lütfen su deposunu doldurun.",
            (Self::Generic, Lang::En) => "Please check on it.",
            (Self::Generic, Lang::De) => "Bitte nach ihm sehen.",
            (Self::Generic, Lang::Tr) => "Lütfen kontrol edin.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [VacuumState; 8] = [
        VacuumState::Idle,
        VacuumState::Cleaning,
        VacuumState::SpotCleaning,
        VacuumState::Returning,
        VacuumState::Docked,
        VacuumState::Paused,
        VacuumState::Error,
        VacuumState::Manual,
    ];

    #[test]
    fn every_state_has_three_language_label_and_advice() {
        for s in ALL_STATES {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!s.label(lang).is_empty(), "{s:?} missing label");
                assert!(!s.advice(lang).is_empty(), "{s:?} missing advice");
            }
        }
    }

    #[test]
    fn every_error_has_three_language_explanation_and_advice() {
        for e in ErrorCode::ALL {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!e.explanation(lang).is_empty(), "{e:?} missing explanation");
                assert!(!e.advice(lang).is_empty(), "{e:?} missing advice");
            }
        }
    }

    #[test]
    fn brush_stuck_reads_grandma_friendly() {
        assert_eq!(ErrorCode::BrushStuck.explanation(Lang::En), "The brush is stuck.");
        assert_eq!(ErrorCode::BrushStuck.advice(Lang::En), "Please free its brush.");
        assert_eq!(VacuumState::Cleaning.label(Lang::En), "Vacuum is cleaning");
    }

    #[test]
    fn cleaning_and_docked_labels_differ() {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            assert_ne!(
                VacuumState::Cleaning.label(lang),
                VacuumState::Docked.label(lang)
            );
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: vacuum UI uses home words only, never vendor/protocol
        // terms. Copied in spirit from cave-home-air-quality / cave-home-lock.
        const BANNED: &[&str] = &[
            "Valetudo", "Xiaomi", "Roborock", "Dreame", "Viomi",
            "MQTT", "REST", "lidar", "topic", "entity_id", "miio",
            "token", "segment", "zone", "API", "node",
            "pod", "kubelet",
        ];
        let mut texts: Vec<String> = Vec::new();
        for s in ALL_STATES {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                texts.push(format!("{} {}", s.label(lang), s.advice(lang)));
            }
        }
        for e in ErrorCode::ALL {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                texts.push(format!("{} {}", e.explanation(lang), e.advice(lang)));
            }
        }
        for text in &texts {
            for banned in BANNED {
                assert!(
                    !text.contains(banned),
                    "UI string leaks jargon {banned:?}: {text}"
                );
            }
        }
    }
}
