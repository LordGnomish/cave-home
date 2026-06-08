//! Grandma-friendly, localised wording for what the lights are doing.
//!
//! The numeric WLED state (brightness bytes, effect ids, segment indices) never
//! reaches the household. The Portal and voice replies speak in plain language:
//! "Living-room lights are warm white at 60%", "Lights off", "Party effect on".
//! This module owns the [`Lang`] enum and the one-line headline builder
//! (Charter §6.3, ADR-007). No protocol jargon is permitted in any string here.

use crate::color::{Hsv, Rgb};

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English.
    En,
    /// German.
    De,
    /// Turkish.
    Tr,
}

/// Convert a 0..=255 WLED brightness byte to a household 0..=100 percentage.
#[must_use]
// The result is `(bri*100+127)/255`, which is at most 100 for `bri == 255`, so
// the final `as u8` never truncates.
#[allow(clippy::cast_possible_truncation)]
pub const fn brightness_percent(bri: u8) -> u8 {
    ((bri as u16 * 100 + 127) / 255) as u8
}

/// A coarse, household-meaningful description of a colour: "warm white",
/// "cool white", "red", "blue"… never a hex code or byte triplet.
#[must_use]
pub fn colour_word(rgb: Rgb, lang: Lang) -> &'static str {
    if rgb.is_black() {
        return match lang {
            Lang::En => "off",
            Lang::De => "aus",
            Lang::Tr => "kapalı",
        };
    }
    let hsv: Hsv = rgb.into();
    // Washed-out, bright tints read as "white" to a household — a warm
    // 2700 K bulb (≈255,220,180) or a cool 6500 K one (≈200,220,255) both look
    // white, just leaning warm/cool. Decide warm vs cool by the red-vs-blue
    // lean. Strongly-coloured light (high saturation) falls through to a hue.
    if hsv.s < 0.45 && hsv.v > 0.5 {
        return if rgb.r >= rgb.b {
            match lang {
                Lang::En => "warm white",
                Lang::De => "warmweiß",
                Lang::Tr => "sıcak beyaz",
            }
        } else {
            match lang {
                Lang::En => "cool white",
                Lang::De => "kaltweiß",
                Lang::Tr => "soğuk beyaz",
            }
        };
    }
    // Otherwise name the hue family.
    match hsv.h {
        h if !(20.0..330.0).contains(&h) => tr_word(lang, "red", "rot", "kırmızı"),
        h if h < 45.0 => tr_word(lang, "orange", "orange", "turuncu"),
        h if h < 70.0 => tr_word(lang, "yellow", "gelb", "sarı"),
        h if h < 165.0 => tr_word(lang, "green", "grün", "yeşil"),
        h if h < 200.0 => tr_word(lang, "cyan", "türkis", "camgöbeği"),
        h if h < 260.0 => tr_word(lang, "blue", "blau", "mavi"),
        h if h < 300.0 => tr_word(lang, "purple", "violett", "mor"),
        _ => tr_word(lang, "pink", "rosa", "pembe"),
    }
}

const fn tr_word(lang: Lang, en: &'static str, de: &'static str, tr: &'static str) -> &'static str {
    match lang {
        Lang::En => en,
        Lang::De => de,
        Lang::Tr => tr,
    }
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
    fn brightness_percent_endpoints_and_midpoint() {
        assert_eq!(brightness_percent(0), 0);
        assert_eq!(brightness_percent(255), 100);
        assert_eq!(brightness_percent(128), 50);
        assert_eq!(brightness_percent(153), 60); // ~60%
    }

    #[test]
    fn colour_word_basic_families() {
        assert_eq!(colour_word(Rgb::BLACK, Lang::En), "off");
        assert_eq!(colour_word(Rgb::new(255, 0, 0), Lang::En), "red");
        assert_eq!(colour_word(Rgb::new(0, 255, 0), Lang::De), "grün");
        assert_eq!(colour_word(Rgb::new(0, 0, 255), Lang::Tr), "mavi");
    }

    #[test]
    fn colour_word_warm_vs_cool_white() {
        assert_eq!(colour_word(Rgb::new(255, 220, 180), Lang::En), "warm white");
        assert_eq!(colour_word(Rgb::new(200, 220, 255), Lang::En), "cool white");
    }

    #[test]
    fn no_jargon_in_colour_words() {
        const BANNED: &[&str] = &["RGB", "hex", "byte", "0x", "#", "fx", "segment"];
        for rgb in [
            Rgb::BLACK,
            Rgb::WHITE,
            Rgb::new(255, 0, 0),
            Rgb::new(0, 0, 255),
            Rgb::new(255, 220, 180),
        ] {
            for l in [Lang::En, Lang::De, Lang::Tr] {
                let w = colour_word(rgb, l);
                for b in BANNED {
                    assert!(!w.contains(b), "colour word leaks jargon {b:?}: {w}");
                }
            }
        }
    }
}
