//! Grandma-friendly, localised labels for the ad/tracker blocker (Charter §6.3,
//! ADR-007, ADR-022).
//!
//! Numbers like rule counts or "RPZ zones" never reach the household. The Portal
//! and mobile app show plain phrases — "Ads and trackers are being blocked",
//! "Blocked 1,240 ads today", "This site is allowed" — localised to the Charter
//! §6.3 mandatory languages EN / DE / TR.

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// Format a count with thousands separators in the household-familiar style
/// (`1240` → `1,240`). Pure string work, no locale crates.
#[must_use]
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// The always-on reassurance line: "Ads and trackers are being blocked".
#[must_use]
pub const fn protection_on(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Ads and trackers are being blocked",
        Lang::De => "Werbung und Tracker werden blockiert",
        Lang::Tr => "Reklamlar ve takip ediciler engelleniyor",
    }
}

/// Shown when the blocker is switched off entirely.
#[must_use]
pub const fn protection_off(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Ad blocking is turned off",
        Lang::De => "Werbeblocker ist ausgeschaltet",
        Lang::Tr => "Reklam engelleme kapalı",
    }
}

/// "Blocked 1,240 ads today" — the headline daily number.
#[must_use]
pub fn blocked_today(count: u64, lang: Lang) -> String {
    let n = group_thousands(count);
    match lang {
        Lang::En => format!("Blocked {n} ads today"),
        Lang::De => format!("Heute {n} Werbungen blockiert"),
        Lang::Tr => format!("Bugün {n} reklam engellendi"),
    }
}

/// Verdict shown for a single site the household looked up.
#[must_use]
pub const fn site_allowed(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "This site is allowed",
        Lang::De => "Diese Seite ist erlaubt",
        Lang::Tr => "Bu site izinli",
    }
}

/// Verdict shown when a site was blocked.
#[must_use]
pub const fn site_blocked(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "This site is blocked",
        Lang::De => "Diese Seite ist blockiert",
        Lang::Tr => "Bu site engellendi",
    }
}

/// Verdict shown when nothing in the rules touched a site.
#[must_use]
pub const fn site_not_filtered(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "This site is not filtered",
        Lang::De => "Diese Seite wird nicht gefiltert",
        Lang::Tr => "Bu site filtrelenmiyor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands_household_style() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(42), "42");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(1_240), "1,240");
        assert_eq!(group_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn blocked_today_carries_grouped_count_in_each_language() {
        assert_eq!(blocked_today(1_240, Lang::En), "Blocked 1,240 ads today");
        assert_eq!(
            blocked_today(1_240, Lang::De),
            "Heute 1,240 Werbungen blockiert"
        );
        assert_eq!(
            blocked_today(1_240, Lang::Tr),
            "Bugün 1,240 reklam engellendi"
        );
    }

    #[test]
    fn every_phrase_has_all_three_languages() {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            assert!(!protection_on(lang).is_empty());
            assert!(!protection_off(lang).is_empty());
            assert!(!site_allowed(lang).is_empty());
            assert!(!site_blocked(lang).is_empty());
            assert!(!site_not_filtered(lang).is_empty());
            assert!(!blocked_today(7, lang).is_empty());
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-022: the UI must never surface DNS/filter internals.
        const BANNED: &[&str] = &[
            "DNS",
            "RPZ",
            "DoH",
            "DoT",
            "MQTT",
            "A record",
            "AAAA",
            "CNAME",
            "BIND",
            "forwarder",
            "ABP",
            "Adblock",
            "hosts file",
            "upstream",
            "resolver",
            "entity_id",
            "pod",
            "kubelet",
        ];
        let mut phrases: Vec<String> = vec![
            blocked_today(1_240, Lang::En),
            blocked_today(1_240, Lang::De),
            blocked_today(1_240, Lang::Tr),
        ];
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            phrases.push(protection_on(lang).to_string());
            phrases.push(protection_off(lang).to_string());
            phrases.push(site_allowed(lang).to_string());
            phrases.push(site_blocked(lang).to_string());
            phrases.push(site_not_filtered(lang).to_string());
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
