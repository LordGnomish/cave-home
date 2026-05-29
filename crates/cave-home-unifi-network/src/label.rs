//! Grandma-friendly phrasing for the home network (Charter §6.3, ADR-007).
//!
//! Nothing in this module ever surfaces a MAC address, an SSID-as-jargon, a
//! port number, "controller", "WebSocket" or "VLAN 30". The household reads
//! "12 devices connected", "Guest Wi-Fi is on", "Kid's tablet is blocked" —
//! localised to EN / DE / TR (the Charter §6.3 mandatory languages from M1).

use crate::summary::{ConnectivitySummary, InternetState};

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// "N devices connected" / "N Geräte verbunden" / "N cihaz bağlı".
#[must_use]
pub fn devices_connected(count: usize, lang: Lang) -> String {
    match lang {
        Lang::En => {
            if count == 1 {
                "1 device connected".to_string()
            } else {
                format!("{count} devices connected")
            }
        }
        Lang::De => {
            if count == 1 {
                "1 Gerät verbunden".to_string()
            } else {
                format!("{count} Geräte verbunden")
            }
        }
        Lang::Tr => format!("{count} cihaz bağlı"),
    }
}

/// "Guest Wi-Fi is on/off" in the three languages.
#[must_use]
pub fn guest_wifi_state(on: bool, lang: Lang) -> &'static str {
    match (on, lang) {
        (true, Lang::En) => "Guest Wi-Fi is on",
        (false, Lang::En) => "Guest Wi-Fi is off",
        (true, Lang::De) => "Gäste-WLAN ist an",
        (false, Lang::De) => "Gäste-WLAN ist aus",
        (true, Lang::Tr) => "Misafir Wi-Fi açık",
        (false, Lang::Tr) => "Misafir Wi-Fi kapalı",
    }
}

/// "<name> is blocked" in the three languages.
#[must_use]
pub fn client_blocked(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{name} is blocked"),
        Lang::De => format!("{name} ist gesperrt"),
        Lang::Tr => format!("{name} engellendi"),
    }
}

/// "<name> is back online" in the three languages.
#[must_use]
pub fn client_unblocked(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{name} is back online"),
        Lang::De => format!("{name} ist wieder online"),
        Lang::Tr => format!("{name} yeniden çevrimiçi"),
    }
}

/// A one-line internet-status sentence.
#[must_use]
pub fn internet_status(state: InternetState, lang: Lang) -> &'static str {
    match (state, lang) {
        (InternetState::Up, Lang::En) => "Internet is up",
        (InternetState::Up, Lang::De) => "Internet ist da",
        (InternetState::Up, Lang::Tr) => "İnternet çalışıyor",
        (InternetState::NoUplink, Lang::En) => "Internet is down",
        (InternetState::NoUplink, Lang::De) => "Internet ist weg",
        (InternetState::NoUplink, Lang::Tr) => "İnternet yok",
        (InternetState::GatewayDown, Lang::En) => "The internet box is offline",
        (InternetState::GatewayDown, Lang::De) => "Die Internet-Box ist offline",
        (InternetState::GatewayDown, Lang::Tr) => "İnternet kutusu kapalı",
    }
}

/// A full at-a-glance household sentence built from a summary, e.g.
/// "12 devices connected. Guest Wi-Fi is on. Internet is up".
#[must_use]
pub fn summary_sentence(summary: &ConnectivitySummary, guest_on: bool, lang: Lang) -> String {
    format!(
        "{}. {}. {}",
        devices_connected(summary.connected_clients, lang),
        guest_wifi_state(guest_on, lang),
        internet_status(summary.internet, lang),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::summarize;

    #[test]
    fn devices_connected_pluralizes() {
        assert_eq!(devices_connected(0, Lang::En), "0 devices connected");
        assert_eq!(devices_connected(1, Lang::En), "1 device connected");
        assert_eq!(devices_connected(12, Lang::En), "12 devices connected");
        assert_eq!(devices_connected(1, Lang::De), "1 Gerät verbunden");
        assert_eq!(devices_connected(3, Lang::De), "3 Geräte verbunden");
        assert_eq!(devices_connected(5, Lang::Tr), "5 cihaz bağlı");
    }

    #[test]
    fn guest_wifi_state_three_langs() {
        assert_eq!(guest_wifi_state(true, Lang::En), "Guest Wi-Fi is on");
        assert_eq!(guest_wifi_state(false, Lang::De), "Gäste-WLAN ist aus");
        assert_eq!(guest_wifi_state(true, Lang::Tr), "Misafir Wi-Fi açık");
    }

    #[test]
    fn blocked_and_unblocked_use_friendly_name() {
        assert_eq!(client_blocked("Kid's tablet", Lang::En), "Kid's tablet is blocked");
        assert_eq!(client_blocked("Kinder-Tablet", Lang::De), "Kinder-Tablet ist gesperrt");
        assert_eq!(client_unblocked("Telefon", Lang::Tr), "Telefon yeniden çevrimiçi");
    }

    #[test]
    fn internet_status_three_langs() {
        assert_eq!(internet_status(InternetState::Up, Lang::En), "Internet is up");
        assert_eq!(internet_status(InternetState::NoUplink, Lang::De), "Internet ist weg");
        assert_eq!(
            internet_status(InternetState::GatewayDown, Lang::Tr),
            "İnternet kutusu kapalı"
        );
    }

    #[test]
    fn summary_sentence_reads_like_a_person() {
        let s = summarize(&[], &[], &[]);
        let sentence = summary_sentence(&s, true, Lang::En);
        assert_eq!(sentence, "0 devices connected. Guest Wi-Fi is on. The internet box is offline");
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-009: the UI must never surface protocol / controller
        // / addressing terms. We scan every user-facing string this module can
        // produce in all three languages.
        const BANNED: &[&str] = &[
            "controller", "WebSocket", "REST", "API", "MAC", "SSID", "VLAN",
            "PoE", "port", "subnet", "uplink", "MQTT", "Zigbee", "entity_id",
            "pod", "kubelet",
        ];
        let names = ["Tablet", "Phone"];
        let mut texts: Vec<String> = Vec::new();
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            texts.push(devices_connected(1, lang));
            texts.push(devices_connected(12, lang));
            texts.push(guest_wifi_state(true, lang).to_string());
            texts.push(guest_wifi_state(false, lang).to_string());
            for st in [InternetState::Up, InternetState::NoUplink, InternetState::GatewayDown] {
                texts.push(internet_status(st, lang).to_string());
            }
            for n in names {
                texts.push(client_blocked(n, lang));
                texts.push(client_unblocked(n, lang));
            }
        }
        for text in texts {
            for banned in BANNED {
                assert!(
                    !text.contains(banned),
                    "UI string leaks jargon {banned:?}: {text:?}"
                );
            }
        }
    }
}
