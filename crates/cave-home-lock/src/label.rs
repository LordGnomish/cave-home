//! Grandma-friendly labels for lock states (Charter §6.3, ADR-007).
//!
//! The end-user never sees `Jammed`, `Unknown` or a vendor's node id — they see
//! "Front door is locked" or "Door is jammed — check it", localised to EN / DE /
//! TR (the Charter §6.3 languages mandatory from M1). This module is the only
//! place lock state becomes words a household reads.

use crate::state::LockState;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl LockState {
    /// A short, plain-language status line for this state — what the household
    /// sees on the door tile. No vendor, protocol or implementation words.
    #[must_use]
    pub const fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Locked, Lang::En) => "Door is locked",
            (Self::Locked, Lang::De) => "Tür ist abgeschlossen",
            (Self::Locked, Lang::Tr) => "Kapı kilitli",
            (Self::Unlocked, Lang::En) => "Door is unlocked",
            (Self::Unlocked, Lang::De) => "Tür ist aufgeschlossen",
            (Self::Unlocked, Lang::Tr) => "Kapı açık (kilitli değil)",
            (Self::Locking, Lang::En) => "Locking the door…",
            (Self::Locking, Lang::De) => "Tür wird abgeschlossen…",
            (Self::Locking, Lang::Tr) => "Kapı kilitleniyor…",
            (Self::Unlocking, Lang::En) => "Unlocking the door…",
            (Self::Unlocking, Lang::De) => "Tür wird aufgeschlossen…",
            (Self::Unlocking, Lang::Tr) => "Kapı açılıyor…",
            (Self::Jammed, Lang::En) => "Door is jammed",
            (Self::Jammed, Lang::De) => "Tür klemmt",
            (Self::Jammed, Lang::Tr) => "Kapı sıkıştı",
            (Self::Open, Lang::En) => "Door is open",
            (Self::Open, Lang::De) => "Tür ist offen",
            (Self::Open, Lang::Tr) => "Kapı açık",
            (Self::Unknown, Lang::En) => "Door status is unclear",
            (Self::Unknown, Lang::De) => "Türstatus ist unklar",
            (Self::Unknown, Lang::Tr) => "Kapı durumu belirsiz",
        }
    }

    /// A concrete, household-level recommended action for this state.
    #[must_use]
    pub const fn advice(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Locked, Lang::En) => "All secure — nothing to do.",
            (Self::Locked, Lang::De) => "Alles sicher — nichts zu tun.",
            (Self::Locked, Lang::Tr) => "Her şey güvende — yapacak bir şey yok.",
            (Self::Unlocked, Lang::En) => "Lock it if everyone is home.",
            (Self::Unlocked, Lang::De) => "Abschließen, wenn alle zu Hause sind.",
            (Self::Unlocked, Lang::Tr) => "Herkes evdeyse kilitleyin.",
            (Self::Locking, Lang::En) => "Just a moment…",
            (Self::Locking, Lang::De) => "Einen Moment…",
            (Self::Locking, Lang::Tr) => "Bir saniye…",
            (Self::Unlocking, Lang::En) => "Just a moment…",
            (Self::Unlocking, Lang::De) => "Einen Moment…",
            (Self::Unlocking, Lang::Tr) => "Bir saniye…",
            (Self::Jammed, Lang::En) => "Go check the door — it could not lock.",
            (Self::Jammed, Lang::De) => "Bitte an der Tür nachsehen — sie konnte nicht schließen.",
            (Self::Jammed, Lang::Tr) => "Kapıyı kontrol edin — kilitlenemedi.",
            (Self::Open, Lang::En) => "Close the door when you are done.",
            (Self::Open, Lang::De) => "Tür schließen, wenn du fertig bist.",
            (Self::Open, Lang::Tr) => "İşiniz bitince kapıyı kapatın.",
            (Self::Unknown, Lang::En) => "Check the door yourself to be sure.",
            (Self::Unknown, Lang::De) => "Zur Sicherheit selbst an der Tür nachsehen.",
            (Self::Unknown, Lang::Tr) => "Emin olmak için kapıyı kendiniz kontrol edin.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [LockState; 7] = [
        LockState::Locked,
        LockState::Unlocked,
        LockState::Locking,
        LockState::Unlocking,
        LockState::Jammed,
        LockState::Open,
        LockState::Unknown,
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
    fn jam_advice_tells_the_user_to_check_the_door() {
        // The safety-critical state must produce an actionable, plain prompt.
        assert_eq!(LockState::Jammed.advice(Lang::En), "Go check the door — it could not lock.");
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: lock UI uses home words only, never vendor/protocol terms.
        const BANNED: &[&str] = &[
            "Nuki", "SwitchBot", "August", "Yale", "Aqara", "ESPHome",
            "Z-Wave", "Zigbee", "Matter", "MQTT", "node", "entity_id",
            "bolt", "latch", "API", "token", "UUID", "REST",
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

    #[test]
    fn locked_and_unlocked_labels_differ() {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            assert_ne!(
                LockState::Locked.label(lang),
                LockState::Unlocked.label(lang)
            );
        }
    }
}
