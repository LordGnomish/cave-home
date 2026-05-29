//! Language tag for the multilingual voice surface (Charter §6.3, ADR-024).
//!
//! cave-home is local-first and multilingual from day one: the same intent
//! engine matches spoken sentences and generates spoken replies in English,
//! German and Turkish. A [`Lang`] threads through template compilation,
//! matching and response generation so a household can speak its own language.

/// A supported voice language. Charter §6.3 makes EN + DE + TR mandatory from
/// the first milestone; more can be added by supplying their sentence sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    /// English.
    En,
    /// German (Deutsch).
    De,
    /// Turkish (Türkçe).
    Tr,
}

impl Lang {
    /// Every language the engine ships with, in a stable order.
    pub const ALL: [Lang; 3] = [Lang::En, Lang::De, Lang::Tr];

    /// The lower-case BCP-47 primary subtag (`"en"`, `"de"`, `"tr"`).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
            Lang::Tr => "tr",
        }
    }

    /// Parse a BCP-47-ish tag (`"en"`, `"EN"`, `"de-DE"`, `"tr_TR"`) into a
    /// [`Lang`]. Returns [`None`] for anything cave-home does not speak yet.
    #[must_use]
    pub fn parse(tag: &str) -> Option<Lang> {
        let primary = tag
            .trim()
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match primary.as_str() {
            "en" => Some(Lang::En),
            "de" => Some(Lang::De),
            "tr" => Some(Lang::Tr),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable() {
        assert_eq!(Lang::En.code(), "en");
        assert_eq!(Lang::De.code(), "de");
        assert_eq!(Lang::Tr.code(), "tr");
    }

    #[test]
    fn parse_handles_region_subtags_and_case() {
        assert_eq!(Lang::parse("en"), Some(Lang::En));
        assert_eq!(Lang::parse("DE"), Some(Lang::De));
        assert_eq!(Lang::parse("tr-TR"), Some(Lang::Tr));
        assert_eq!(Lang::parse("de_DE"), Some(Lang::De));
        assert_eq!(Lang::parse("fr"), None);
        assert_eq!(Lang::parse(""), None);
    }

    #[test]
    fn all_covers_every_variant() {
        assert_eq!(Lang::ALL.len(), 3);
        assert!(Lang::ALL.contains(&Lang::Tr));
    }
}
