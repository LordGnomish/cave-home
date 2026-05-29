//! Grandma-friendly, localised messages (Charter §6.3, ADR-007).
//!
//! The household sees these strings; the engine's internal vocabulary
//! ("config entry", "platform forward", "setup retry", a protocol name) must
//! **never** leak here. Every message is plain household language in EN / DE /
//! TR (the Charter §6.3 mandatory-from-M1 languages).

use crate::capability::Capability;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// "Found a new <thing> — add it?" — the discovery suggestion shown when a
/// genuinely new device appears on the network.
#[must_use]
pub fn found_new(cap: Capability, lang: Lang) -> String {
    let noun = cap.noun(lang);
    match lang {
        Lang::En => format!("Found a new {noun} — add it?"),
        Lang::De => format!("Neue(r/s) {noun} gefunden — hinzufügen?"),
        Lang::Tr => format!("Yeni bir {noun} bulundu — eklensin mi?"),
    }
}

/// "<Name> connected" — shown when a thing reaches the running state.
#[must_use]
pub fn connected(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{name} connected"),
        Lang::De => format!("{name} verbunden"),
        Lang::Tr => format!("{name} bağlandı"),
    }
}

/// "Couldn't connect — we'll keep trying" — the message for a *transient*
/// failure where the engine will retry on its own.
#[must_use]
pub fn retrying(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Couldn't connect to {name} — we'll keep trying"),
        Lang::De => format!("Verbindung zu {name} fehlgeschlagen — wir versuchen es weiter"),
        Lang::Tr => format!("{name} bağlanamadı — denemeye devam edeceğiz"),
    }
}

/// "Couldn't set <Name> up — please check it" — the message for a *permanent*
/// failure that needs the household to do something (wrong password, etc.).
#[must_use]
pub fn needs_attention(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Couldn't set {name} up — please check it"),
        Lang::De => format!("{name} konnte nicht eingerichtet werden — bitte überprüfen"),
        Lang::Tr => format!("{name} kurulamadı — lütfen kontrol edin"),
    }
}

/// "<Name> removed" — shown after the household removes a thing.
#[must_use]
pub fn removed(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{name} removed"),
        Lang::De => format!("{name} entfernt"),
        Lang::Tr => format!("{name} kaldırıldı"),
    }
}

/// "<Name> is already added" — shown when discovery re-finds a known device.
#[must_use]
pub fn already_added(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{name} is already added"),
        Lang::De => format!("{name} ist bereits hinzugefügt"),
        Lang::Tr => format!("{name} zaten ekli"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_new_reads_like_a_person_wrote_it() {
        assert_eq!(found_new(Capability::Light, Lang::En), "Found a new light — add it?");
        assert_eq!(connected("Living-room hub", Lang::En), "Living-room hub connected");
        assert_eq!(
            retrying("Living-room hub", Lang::En),
            "Couldn't connect to Living-room hub — we'll keep trying"
        );
    }

    #[test]
    fn all_messages_exist_in_three_languages() {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            assert!(!found_new(Capability::Camera, lang).is_empty());
            assert!(!connected("Hub", lang).is_empty());
            assert!(!retrying("Hub", lang).is_empty());
            assert!(!needs_attention("Hub", lang).is_empty());
            assert!(!removed("Hub", lang).is_empty());
            assert!(!already_added("Hub", lang).is_empty());
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: no engine/protocol vocabulary may reach a household.
        const BANNED: &[&str] = &[
            "config entry",
            "config_entry",
            "integration domain",
            "platform forward",
            "platform",
            "entity",
            "topological",
            "iot-class",
            "iot_class",
            "setup_retry",
            "setup retry",
            "setuperror",
            "MQTT",
            "mDNS",
            "SSDP",
            "DHCP",
            "Zigbee",
            "unique-id",
            "unique_id",
            "pod",
            "kubelet",
        ];
        let samples = [
            found_new(Capability::Light, Lang::En),
            found_new(Capability::Light, Lang::De),
            found_new(Capability::Light, Lang::Tr),
            connected("Hub", Lang::En),
            connected("Hub", Lang::De),
            connected("Hub", Lang::Tr),
            retrying("Hub", Lang::En),
            retrying("Hub", Lang::De),
            retrying("Hub", Lang::Tr),
            needs_attention("Hub", Lang::En),
            needs_attention("Hub", Lang::De),
            needs_attention("Hub", Lang::Tr),
            removed("Hub", Lang::En),
            already_added("Hub", Lang::En),
        ];
        for s in &samples {
            let low = s.to_lowercase();
            for b in BANNED {
                assert!(
                    !low.contains(&b.to_lowercase()),
                    "message leaks jargon {b:?}: {s}"
                );
            }
        }
    }

    #[test]
    fn every_capability_produces_a_clean_suggestion() {
        for cap in Capability::all() {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let msg = found_new(*cap, lang);
                assert!(msg.contains(cap.noun(lang)));
            }
        }
    }
}
