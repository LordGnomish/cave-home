// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Grandma-friendly access messages (Charter §6.3, ADR-007 / ADR-009).
//!
//! A household never reads "DPS relay", "OSDP reader" or "credential provider".
//! They read "Front door unlocked for 30 seconds", "Door held open — please
//! close it", "Access denied — outside allowed hours" — localised to EN / DE /
//! TR (the Charter §6.3 languages mandatory from M1). This module is the only
//! place door-control state turns into words a household reads.

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    /// English.
    En,
    /// German.
    De,
    /// Turkish.
    Tr,
}

impl Lang {
    /// Every supported language, for exhaustive iteration in callers & tests.
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::En, Self::De, Self::Tr]
    }
}

/// A household-facing message about a door. Rendered to plain words by
/// [`AccessMessage::text`]; the variants carry only the facts, never jargon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessMessage {
    /// A door was unlocked indefinitely.
    Unlocked {
        /// The door's friendly name, e.g. "Front door".
        door: String,
    },
    /// A door was locked.
    Locked {
        /// The door's friendly name.
        door: String,
    },
    /// A door was temporarily unlocked and will re-lock after `seconds`.
    TempUnlocked {
        /// The door's friendly name.
        door: String,
        /// How many seconds it stays unlocked.
        seconds: u32,
    },
    /// A door has been held open longer than is safe.
    HeldOpen {
        /// The door's friendly name.
        door: String,
    },
    /// Everywhere is being held unlocked for an emergency exit.
    Evacuation,
    /// Everywhere is being held locked to keep people out.
    Lockdown,
    /// Access was granted.
    AccessGranted {
        /// The door's friendly name.
        door: String,
    },
    /// Access was refused because it is outside the person's allowed hours.
    DeniedOutsideHours,
    /// Access was refused because the person is not allowed through this door.
    DeniedNoPermission,
    /// Access was refused because everywhere is locked down.
    DeniedLockdown,
    /// Access was refused because the key/card/PIN was not recognised.
    DeniedUnknown,
}

impl AccessMessage {
    /// Render this message to a complete, plain-language sentence.
    #[must_use]
    pub fn text(&self, lang: Lang) -> String {
        match self {
            Self::Unlocked { door } => match lang {
                Lang::En => format!("{door} unlocked."),
                Lang::De => format!("{door} aufgeschlossen."),
                Lang::Tr => format!("{door} açıldı."),
            },
            Self::Locked { door } => match lang {
                Lang::En => format!("{door} locked."),
                Lang::De => format!("{door} abgeschlossen."),
                Lang::Tr => format!("{door} kilitlendi."),
            },
            Self::TempUnlocked { door, seconds } => match lang {
                Lang::En => format!("{door} unlocked for {seconds} seconds."),
                Lang::De => format!("{door} für {seconds} Sekunden aufgeschlossen."),
                Lang::Tr => format!("{door} {seconds} saniyeliğine açıldı."),
            },
            Self::HeldOpen { door } => match lang {
                Lang::En => format!("{door} held open — please close it."),
                Lang::De => format!("{door} steht offen — bitte schließen."),
                Lang::Tr => format!("{door} açık kaldı — lütfen kapatın."),
            },
            Self::Evacuation => match lang {
                Lang::En => "Every door is open so everyone can get out safely.".to_string(),
                Lang::De => "Alle Türen sind offen, damit alle sicher hinaus können.".to_string(),
                Lang::Tr => "Herkes güvenle çıkabilsin diye tüm kapılar açık.".to_string(),
            },
            Self::Lockdown => match lang {
                Lang::En => "Every door is locked to keep everyone safe inside.".to_string(),
                Lang::De => "Alle Türen sind abgeschlossen, um alle drinnen zu schützen.".to_string(),
                Lang::Tr => "Herkesi içeride güvende tutmak için tüm kapılar kilitli.".to_string(),
            },
            Self::AccessGranted { door } => match lang {
                Lang::En => format!("Welcome — {door} is open for you."),
                Lang::De => format!("Willkommen — {door} ist für dich offen."),
                Lang::Tr => format!("Hoş geldiniz — {door} sizin için açık."),
            },
            Self::DeniedOutsideHours => match lang {
                Lang::En => "Access denied — outside allowed hours.".to_string(),
                Lang::De => "Zugang verweigert — außerhalb der erlaubten Zeiten.".to_string(),
                Lang::Tr => "Erişim reddedildi — izin verilen saatlerin dışında.".to_string(),
            },
            Self::DeniedNoPermission => match lang {
                Lang::En => "Access denied — not allowed through this door.".to_string(),
                Lang::De => "Zugang verweigert — für diese Tür nicht berechtigt.".to_string(),
                Lang::Tr => "Erişim reddedildi — bu kapı için izniniz yok.".to_string(),
            },
            Self::DeniedLockdown => match lang {
                Lang::En => "Access denied — the house is locked down right now.".to_string(),
                Lang::De => "Zugang verweigert — das Haus ist gerade abgeriegelt.".to_string(),
                Lang::Tr => "Erişim reddedildi — ev şu anda güvenlik kilidinde.".to_string(),
            },
            Self::DeniedUnknown => match lang {
                Lang::En => "Access denied — that key was not recognised.".to_string(),
                Lang::De => "Zugang verweigert — der Schlüssel wurde nicht erkannt.".to_string(),
                Lang::Tr => "Erişim reddedildi — anahtar tanınmadı.".to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_messages() -> Vec<AccessMessage> {
        vec![
            AccessMessage::Unlocked { door: "Front door".into() },
            AccessMessage::Locked { door: "Front door".into() },
            AccessMessage::TempUnlocked { door: "Front door".into(), seconds: 30 },
            AccessMessage::HeldOpen { door: "Back door".into() },
            AccessMessage::Evacuation,
            AccessMessage::Lockdown,
            AccessMessage::AccessGranted { door: "Front door".into() },
            AccessMessage::DeniedOutsideHours,
            AccessMessage::DeniedNoPermission,
            AccessMessage::DeniedLockdown,
            AccessMessage::DeniedUnknown,
        ]
    }

    #[test]
    fn every_message_renders_non_empty_in_all_languages() {
        for m in sample_messages() {
            for lang in Lang::all() {
                assert!(!m.text(lang).is_empty(), "{m:?} empty in {lang:?}");
            }
        }
    }

    #[test]
    fn temp_unlock_states_the_duration() {
        let m = AccessMessage::TempUnlocked { door: "Front door".into(), seconds: 30 };
        assert!(m.text(Lang::En).contains("30 seconds"));
        assert!(m.text(Lang::De).contains("30 Sekunden"));
        assert!(m.text(Lang::Tr).contains("30 saniye"));
    }

    #[test]
    fn held_open_prompts_the_user_to_close_it() {
        // Safety-critical wording: must be an actionable plain prompt.
        let m = AccessMessage::HeldOpen { door: "Front door".into() };
        assert_eq!(m.text(Lang::En), "Front door held open — please close it.");
    }

    #[test]
    fn denial_reasons_render_distinct_text() {
        let hours = AccessMessage::DeniedOutsideHours.text(Lang::En);
        let perm = AccessMessage::DeniedNoPermission.text(Lang::En);
        let lock = AccessMessage::DeniedLockdown.text(Lang::En);
        let unk = AccessMessage::DeniedUnknown.text(Lang::En);
        assert_ne!(hours, perm);
        assert_ne!(perm, lock);
        assert_ne!(lock, unk);
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: door UI uses home words only, never vendor/protocol
        // or hardware terms.
        const BANNED: &[&str] = &[
            "DPS", "relay", "OSDP", "reader", "MQTT", "entity_id", "Ubiquiti",
            "UniFi", "REST", "WebSocket", "API", "token", "GPIO", "NFC", "PIN",
            "credential", "hub", "tamper", "anti-passback", "lockdown mode",
            "GUID", "UUID", "schedule window", "capability",
        ];
        for m in sample_messages() {
            for lang in Lang::all() {
                let text = m.text(lang);
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "message {m:?} leaks jargon {banned:?} in {lang:?}: {text}"
                    );
                }
            }
        }
    }
}
