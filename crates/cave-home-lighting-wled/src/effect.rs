//! Effect and palette registries.
//!
//! WLED ships a large catalogue of built-in animations ("effects") and colour
//! gradients ("palettes"), each addressed in the JSON API by a small integer
//! id (`fx` / `pal`). The raw catalogue uses developer/community names —
//! "Solid", "Akemi", "Aurora", "Saw" — which are meaningless to a household.
//!
//! This module curates a useful subset of the *documented, stable* built-in
//! effects and palettes and pairs each id with a grandma-friendly localised
//! name (EN/DE/TR). The numeric id is the wire value; the friendly name is what
//! the Portal shows. Implemented from the public WLED JSON API effect/palette
//! list; firmware source was not read (ADR-014 clean-room). The full 100+
//! enumeration is deferred to Phase 1b (see parity manifest).

use crate::label::Lang;

/// A built-in WLED effect: its wire id plus localised household names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    /// The WLED `fx` id sent over the JSON API.
    pub id: u8,
    en: &'static str,
    de: &'static str,
    tr: &'static str,
}

impl Effect {
    /// The grandma-friendly name in the requested language.
    #[must_use]
    pub const fn name(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::De => self.de,
            Lang::Tr => self.tr,
        }
    }
}

/// A built-in WLED palette: its wire id plus localised household names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The WLED `pal` id sent over the JSON API.
    pub id: u8,
    en: &'static str,
    de: &'static str,
    tr: &'static str,
}

impl Palette {
    /// The grandma-friendly name in the requested language.
    #[must_use]
    pub const fn name(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::De => self.de,
            Lang::Tr => self.tr,
        }
    }
}

macro_rules! effect {
    ($id:expr, $en:expr, $de:expr, $tr:expr) => {
        Effect { id: $id, en: $en, de: $de, tr: $tr }
    };
}

macro_rules! palette {
    ($id:expr, $en:expr, $de:expr, $tr:expr) => {
        Palette { id: $id, en: $en, de: $de, tr: $tr }
    };
}

/// A curated subset of the documented built-in WLED effects, ids matching the
/// public effect index. Friendly names replace the raw developer names.
pub const EFFECTS: &[Effect] = &[
    effect!(0, "Steady colour", "Ruhiges Licht", "Sabit renk"),
    effect!(1, "Slow blink", "Langsames Blinken", "Yavaş yanıp sönme"),
    effect!(2, "Breathing", "Atmen", "Nefes alma"),
    effect!(3, "Colour wipe", "Farbverlauf", "Renk dalgası"),
    effect!(8, "Soft glow", "Sanftes Leuchten", "Yumuşak parıltı"),
    effect!(9, "Rainbow", "Regenbogen", "Gökkuşağı"),
    effect!(12, "Twinkle", "Funkeln", "Pırıltı"),
    effect!(13, "Sparkle", "Glitzern", "Kıvılcım"),
    effect!(15, "Running lights", "Lauflicht", "Yürüyen ışıklar"),
    effect!(23, "Shimmer", "Schimmern", "Işıltı"),
    effect!(38, "Fire flicker", "Kaminfeuer", "Şömine ateşi"),
    effect!(41, "Theatre chase", "Lichterkette", "Tiyatro ışıkları"),
    effect!(43, "Colour loop", "Farbschleife", "Renk döngüsü"),
    effect!(57, "Lightning", "Blitze", "Şimşek"),
    effect!(63, "Police lights", "Polizeilicht", "Polis ışıkları"),
    effect!(66, "Cosy fire", "Gemütliches Feuer", "Sıcacık ateş"),
    effect!(67, "Sunrise", "Sonnenaufgang", "Gün doğumu"),
    effect!(73, "Party", "Party", "Parti"),
    effect!(74, "Festive colours", "Festtagsfarben", "Bayram renkleri"),
    effect!(80, "Twinkling stars", "Funkelnde Sterne", "Yıldız parıltısı"),
    effect!(87, "Glitter", "Glitzerregen", "Sim yağmuru"),
    effect!(88, "Candle", "Kerze", "Mum ışığı"),
    effect!(89, "Fireworks", "Feuerwerk", "Havai fişek"),
    effect!(101, "Gentle waves", "Sanfte Wellen", "Yumuşak dalgalar"),
    effect!(110, "Flowing colours", "Fließende Farben", "Akan renkler"),
    effect!(115, "Aurora", "Polarlicht", "Kutup ışıkları"),
];

/// A curated subset of the documented built-in WLED palettes, ids matching the
/// public palette index. Friendly names replace the raw developer names.
pub const PALETTES: &[Palette] = &[
    palette!(0, "Match the light colour", "Lichtfarbe übernehmen", "Işık rengini kullan"),
    palette!(1, "Random colours", "Zufällige Farben", "Rastgele renkler"),
    palette!(2, "Primary colour", "Hauptfarbe", "Ana renk"),
    palette!(3, "Three chosen colours", "Drei Wunschfarben", "Seçili üç renk"),
    palette!(6, "Rainbow", "Regenbogen", "Gökkuşağı"),
    palette!(8, "Party", "Party", "Parti"),
    palette!(9, "Ocean", "Ozean", "Okyanus"),
    palette!(10, "Forest", "Wald", "Orman"),
    palette!(11, "Rainbow ring", "Regenbogenkreis", "Gökkuşağı halkası"),
    palette!(35, "Warm sunset", "Warmer Sonnenuntergang", "Sıcak gün batımı"),
    palette!(38, "Cosy fire", "Gemütliches Feuer", "Sıcacık ateş"),
    palette!(43, "Beach", "Strand", "Sahil"),
    palette!(47, "Springtime", "Frühling", "İlkbahar"),
    palette!(58, "Candlelight", "Kerzenlicht", "Mum ışığı"),
];

/// The largest effect id this crate validates against.
///
/// Commands accept any id in `0..=MAX_EFFECT_ID`; the registry above is for
/// *naming*, this bound is for *validation*.
pub const MAX_EFFECT_ID: u8 = 200;

/// The largest palette id this curated registry validates against.
pub const MAX_PALETTE_ID: u8 = 70;

/// Look up the friendly metadata for an effect id, if it is in the curated set.
#[must_use]
pub fn effect(id: u8) -> Option<&'static Effect> {
    EFFECTS.iter().find(|e| e.id == id)
}

/// Look up the friendly metadata for a palette id, if it is in the curated set.
#[must_use]
pub fn palette(id: u8) -> Option<&'static Palette> {
    PALETTES.iter().find(|p| p.id == id)
}

/// The friendly effect name for an id, falling back to a neutral household
/// phrase for ids outside the curated set (never the raw developer name).
#[must_use]
pub fn effect_name(id: u8, lang: Lang) -> &'static str {
    effect(id).map_or_else(
        || match lang {
            Lang::En => "Light effect",
            Lang::De => "Lichteffekt",
            Lang::Tr => "Işık efekti",
        },
        |e| e.name(lang),
    )
}

#[cfg(test)]
mod tests {
    // Tests legitimately use expect/unwrap on known-good inputs and the
    // `let mut s = Default; s.field = ..` setup shape; these patterns are fine
    // in test scaffolding even though clippy::pedantic flags them in shipped code.
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::field_reassign_with_default,
        clippy::uninlined_format_args,
        clippy::float_cmp
    )]
    use super::*;

    #[test]
    fn registries_are_non_trivial_and_id_sorted() {
        assert!(EFFECTS.len() >= 20, "curated effect list should be useful");
        assert!(PALETTES.len() >= 10, "curated palette list should be useful");
        // ids are unique and ascending so lookup stays predictable.
        for w in EFFECTS.windows(2) {
            assert!(w[0].id < w[1].id, "effect ids must ascend: {:?}", w);
        }
        for w in PALETTES.windows(2) {
            assert!(w[0].id < w[1].id, "palette ids must ascend: {:?}", w);
        }
    }

    #[test]
    fn lookup_known_and_unknown() {
        assert_eq!(effect(73).map(|e| e.name(Lang::En)), Some("Party"));
        assert_eq!(effect(0).map(|e| e.name(Lang::En)), Some("Steady colour"));
        assert!(effect(199).is_none());
        assert_eq!(palette(6).map(|p| p.name(Lang::Tr)), Some("Gökkuşağı"));
        assert!(palette(255).is_none());
    }

    #[test]
    fn effect_name_falls_back_friendly_not_raw() {
        // An id we have no friendly name for must still be friendly, never a
        // raw developer token leaked to the household.
        assert_eq!(effect_name(199, Lang::En), "Light effect");
        assert_eq!(effect_name(199, Lang::De), "Lichteffekt");
        assert_eq!(effect_name(199, Lang::Tr), "Işık efekti");
    }

    #[test]
    fn every_entry_has_all_three_languages() {
        for e in EFFECTS {
            for l in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!e.name(l).is_empty(), "effect {} missing a language", e.id);
            }
        }
        for p in PALETTES {
            for l in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!p.name(l).is_empty(), "palette {} missing a language", p.id);
            }
        }
    }

    #[test]
    fn names_are_not_raw_developer_tokens() {
        // A spot-check that we replaced the technical upstream names.
        const RAW: &[&str] = &["Akemi", "Saw", "Solid", "FX", "Chase Flash"];
        for e in EFFECTS {
            for l in [Lang::En, Lang::De, Lang::Tr] {
                let n = e.name(l);
                for raw in RAW {
                    assert_ne!(n, *raw, "effect {} leaks raw name", e.id);
                }
            }
        }
    }
}
