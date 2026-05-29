//! Grandma-friendly, localised labels for the local DNS resolver (Charter §6.3,
//! ADR-007, ADR-022).
//!
//! Resolver internals — local-zone types, forward zones, CIDR access rules —
//! never reach the household. The Portal and mobile app show plain phrases:
//! "Local devices found by name", "This name points to your printer", "Outside
//! lookups are private" — localised to the Charter §6.3 mandatory languages
//! EN / DE / TR.

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// Headline for the "your home knows its own devices by name" feature.
#[must_use]
pub const fn local_names_on(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Local devices found by name",
        Lang::De => "Geräte zu Hause werden über ihren Namen gefunden",
        Lang::Tr => "Yerel cihazlar adıyla bulunuyor",
    }
}

/// Shown next to a name that resolves to a known household device, e.g. the
/// printer. The device label is supplied by the caller (already friendly).
#[must_use]
pub fn name_points_to(device: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("This name points to your {device}"),
        Lang::De => format!("Dieser Name verweist auf Ihren {device}"),
        Lang::Tr => format!("Bu ad {device} cihazınıza işaret ediyor"),
    }
}

/// Reassurance that lookups for the wider internet stay in the home.
#[must_use]
pub const fn outside_lookups_private(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Outside lookups are private",
        Lang::De => "Auswärtige Suchen bleiben privat",
        Lang::Tr => "Dışarıya yapılan aramalar gizli kalır",
    }
}

/// Shown when a name was deliberately not answered (blocked / refused locally).
#[must_use]
pub const fn name_not_answered(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "This name is not answered at home",
        Lang::De => "Dieser Name wird zu Hause nicht beantwortet",
        Lang::Tr => "Bu ad evde yanıtlanmıyor",
    }
}

/// Shown when a guest device is not allowed to ask the home resolver.
#[must_use]
pub const fn device_not_allowed(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "This device is not allowed to look up names",
        Lang::De => "Dieses Gerät darf keine Namen nachschlagen",
        Lang::Tr => "Bu cihazın ad araması yapmasına izin yok",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phrase_has_all_three_languages() {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            assert!(!local_names_on(lang).is_empty());
            assert!(!outside_lookups_private(lang).is_empty());
            assert!(!name_not_answered(lang).is_empty());
            assert!(!device_not_allowed(lang).is_empty());
            assert!(!name_points_to("printer", lang).is_empty());
        }
    }

    #[test]
    fn name_points_to_includes_device() {
        assert_eq!(
            name_points_to("printer", Lang::En),
            "This name points to your printer"
        );
        assert!(name_points_to("Drucker", Lang::De).contains("Drucker"));
        assert!(name_points_to("yazıcı", Lang::Tr).contains("yazıcı"));
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-022: the UI must never surface resolver internals.
        const BANNED: &[&str] = &[
            "DNS",
            "local-zone",
            "forward-zone",
            "stub-zone",
            "NXDOMAIN",
            "PTR",
            "CIDR",
            "DoT",
            "DoH",
            "MQTT",
            "A record",
            "AAAA",
            "CNAME",
            "upstream",
            "resolver",
            "recursion",
            "TTL",
            "entity_id",
            "pod",
            "kubelet",
        ];
        let mut phrases: Vec<String> = vec![
            name_points_to("printer", Lang::En),
            name_points_to("Drucker", Lang::De),
            name_points_to("yazıcı", Lang::Tr),
        ];
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            phrases.push(local_names_on(lang).to_string());
            phrases.push(outside_lookups_private(lang).to_string());
            phrases.push(name_not_answered(lang).to_string());
            phrases.push(device_not_allowed(lang).to_string());
        }
        for text in &phrases {
            for banned in BANNED {
                assert!(
                    !text.contains(banned),
                    "UI phrase leaks jargon {banned:?}: {text}"
                );
            }
        }
    }
}
