//! Grandma-friendly labels for alarm-panel states (Charter §6.3, ADR-007).
//!
//! The end-user never sees `ArmedCustomBypass`, `Pending` or a vendor's panel
//! model — they see "Alarm is on — away mode" or "Alarm going off — check the
//! house", localised to EN / DE / TR (the Charter §6.3 languages mandatory from
//! M1). This module is the only place alarm state becomes words a household
//! reads.

use crate::state::AlarmState;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl AlarmState {
    /// A short, plain-language status line for this state — what the household
    /// sees on the alarm tile. No vendor, protocol or implementation words.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Disarmed, Lang::En) => "Alarm is off",
            (Self::Disarmed, Lang::De) => "Alarm ist aus",
            (Self::Disarmed, Lang::Tr) => "Alarm kapalı",
            (Self::ArmedHome, Lang::En) => "Alarm is on — home mode",
            (Self::ArmedHome, Lang::De) => "Alarm ist an — Modus Zuhause",
            (Self::ArmedHome, Lang::Tr) => "Alarm açık — evde modu",
            (Self::ArmedAway, Lang::En) => "Alarm is on — away mode",
            (Self::ArmedAway, Lang::De) => "Alarm ist an — Modus Abwesend",
            (Self::ArmedAway, Lang::Tr) => "Alarm açık — dışarıda modu",
            (Self::ArmedNight, Lang::En) => "Alarm is on — night mode",
            (Self::ArmedNight, Lang::De) => "Alarm ist an — Modus Nacht",
            (Self::ArmedNight, Lang::Tr) => "Alarm açık — gece modu",
            (Self::ArmedVacation, Lang::En) => "Alarm is on — vacation mode",
            (Self::ArmedVacation, Lang::De) => "Alarm ist an — Modus Urlaub",
            (Self::ArmedVacation, Lang::Tr) => "Alarm açık — tatil modu",
            (Self::ArmedCustomBypass, Lang::En) => "Alarm is on — some areas off",
            (Self::ArmedCustomBypass, Lang::De) => "Alarm ist an — einige Bereiche aus",
            (Self::ArmedCustomBypass, Lang::Tr) => "Alarm açık — bazı bölümler kapalı",
            (Self::Arming, Lang::En) => "Turning the alarm on — time to leave",
            (Self::Arming, Lang::De) => "Alarm wird eingeschaltet — Zeit zu gehen",
            (Self::Arming, Lang::Tr) => "Alarm açılıyor — çıkma zamanı",
            (Self::Pending, Lang::En) => "Welcome home — enter your code",
            (Self::Pending, Lang::De) => "Willkommen zu Hause — Code eingeben",
            (Self::Pending, Lang::Tr) => "Hoş geldiniz — kodunuzu girin",
            (Self::Triggered, Lang::En) => "Alarm is going off",
            (Self::Triggered, Lang::De) => "Der Alarm geht los",
            (Self::Triggered, Lang::Tr) => "Alarm çalıyor",
        }
    }

    /// A concrete, household-level recommended action for this state.
    #[must_use]
    pub const fn advice(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Disarmed, Lang::En) => "Turn it on when you head out or go to bed.",
            (Self::Disarmed, Lang::De) => "Schalte ihn ein, wenn du gehst oder schlafen gehst.",
            (Self::Disarmed, Lang::Tr) => "Çıkarken ya da yatarken açın.",
            (Self::ArmedHome, Lang::En) => "All set — the doors and windows are watched.",
            (Self::ArmedHome, Lang::De) => "Alles bereit — Türen und Fenster werden überwacht.",
            (Self::ArmedHome, Lang::Tr) => "Her şey hazır — kapı ve pencereler izleniyor.",
            (Self::ArmedAway, Lang::En) => "The whole house is watched — enjoy your day.",
            (Self::ArmedAway, Lang::De) => "Das ganze Haus wird überwacht — schönen Tag.",
            (Self::ArmedAway, Lang::Tr) => "Evin tamamı izleniyor — iyi günler.",
            (Self::ArmedNight, Lang::En) => "Sleep well — the house is being watched.",
            (Self::ArmedNight, Lang::De) => "Schlaf gut — das Haus wird überwacht.",
            (Self::ArmedNight, Lang::Tr) => "İyi uykular — ev izleniyor.",
            (Self::ArmedVacation, Lang::En) => "Have a good trip — the house is fully watched.",
            (Self::ArmedVacation, Lang::De) => "Gute Reise — das Haus wird voll überwacht.",
            (Self::ArmedVacation, Lang::Tr) => "İyi yolculuklar — ev tamamen izleniyor.",
            (Self::ArmedCustomBypass, Lang::En) => "On, but a few areas are left off on purpose.",
            (Self::ArmedCustomBypass, Lang::De) => "An, aber ein paar Bereiche sind bewusst aus.",
            (Self::ArmedCustomBypass, Lang::Tr) => "Açık, ama birkaç bölüm bilerek kapalı.",
            (Self::Arming, Lang::En) => "Leave the house before the time runs out.",
            (Self::Arming, Lang::De) => "Verlasse das Haus, bevor die Zeit abläuft.",
            (Self::Arming, Lang::Tr) => "Süre dolmadan evden çıkın.",
            (Self::Pending, Lang::En) => "Enter your code now to turn the alarm off.",
            (Self::Pending, Lang::De) => "Gib jetzt deinen Code ein, um den Alarm auszuschalten.",
            (Self::Pending, Lang::Tr) => "Alarmı kapatmak için şimdi kodunuzu girin.",
            (Self::Triggered, Lang::En) => "Check the house and turn the alarm off with your code.",
            (Self::Triggered, Lang::De) => "Sieh im Haus nach und schalte den Alarm mit deinem Code aus.",
            (Self::Triggered, Lang::Tr) => "Evi kontrol edin ve kodunuzla alarmı kapatın.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [AlarmState; 9] = [
        AlarmState::Disarmed,
        AlarmState::ArmedHome,
        AlarmState::ArmedAway,
        AlarmState::ArmedNight,
        AlarmState::ArmedVacation,
        AlarmState::ArmedCustomBypass,
        AlarmState::Arming,
        AlarmState::Pending,
        AlarmState::Triggered,
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
    fn triggered_advice_tells_the_user_to_check_the_house() {
        // The safety-critical state must produce an actionable, plain prompt.
        assert_eq!(
            AlarmState::Triggered.advice(Lang::En),
            "Check the house and turn the alarm off with your code."
        );
        assert_eq!(AlarmState::Triggered.label(Lang::En), "Alarm is going off");
    }

    #[test]
    fn armed_modes_have_distinct_labels() {
        // A household must be able to tell away from night from home at a glance.
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            let away = AlarmState::ArmedAway.label(lang);
            let home = AlarmState::ArmedHome.label(lang);
            let night = AlarmState::ArmedNight.label(lang);
            assert_ne!(away, home);
            assert_ne!(home, night);
            assert_ne!(away, night);
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: alarm UI uses home words only, never vendor/protocol or
        // state-machine terms. "the house" / "away mode" — never "zone GPIO",
        // "MQTT", "entity_id".
        const BANNED: &[&str] = &[
            "AlarmDecoder", "Honeywell", "DSC", "Bosch", "ELK", "Z-Wave",
            "Zigbee", "MQTT", "GPIO", "entity_id", "Pending", "Triggered",
            "Disarmed", "Armed", "node", "API", "token", "sensor", "Sensor",
            "RTSP", "USB", "panel", "Panel",
        ];
        for s in ALL_STATES {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let text = format!("{} {}", s.label(lang), s.advice(lang));
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "state {s:?} leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
