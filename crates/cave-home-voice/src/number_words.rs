//! Spoken-number parsing — turn "fifty", "5", or "twenty five" into `50`, `5`,
//! `25` so a brightness/temperature slot carries a real number.
//!
//! # Scope
//!
//! cave-home's voice commands need numbers in a small, bounded range:
//! brightness percentages (0–100) and household temperatures (roughly 0–35).
//! This parser therefore covers `0..=100` in words, plus bare digit strings of
//! any size. It is deliberately *not* a general number-to-words library.
//!
//! English is fully spelled out. German and Turkish number words are covered
//! up to the same range so the multilingual command sets (Charter §6.3) work;
//! the long-form compound rules of those languages above 100 are out of scope
//! for Phase 1 and digit input always works regardless of language.

use crate::label::Lang;

/// Parse a single spoken-number token-run (already lower-cased, words joined by
/// single spaces) into a non-negative integer.
///
/// Accepts:
/// - bare digits: `"42"` → `Some(42)`
/// - English words: `"forty two"`, `"fifty"`, `"one hundred"` → `42`, `50`, `100`
/// - German words: `"fünfzig"`, `"zweiundvierzig"` → `50`, `42`
/// - Turkish words: `"elli"`, `"kırk iki"` → `50`, `42`
///
/// Returns [`None`] if the text is not a number cave-home recognises.
#[must_use]
pub fn parse_number(text: &str, lang: Lang) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // Bare digits first — language-independent.
    if text.chars().all(|c| c.is_ascii_digit()) {
        return text.parse::<u32>().ok();
    }
    match lang {
        Lang::En => parse_words_en(text),
        Lang::De => parse_words_de(text),
        Lang::Tr => parse_words_tr(text),
    }
}

fn unit_en(word: &str) -> Option<u32> {
    Some(match word {
        "zero" => 0,
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        _ => return None,
    })
}

fn ten_en(word: &str) -> Option<u32> {
    Some(match word {
        "twenty" => 20,
        "thirty" => 30,
        "forty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        _ => return None,
    })
}

fn parse_words_en(text: &str) -> Option<u32> {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        ["one", "hundred"] | ["hundred"] => Some(100),
        [w] => unit_en(w).or_else(|| ten_en(w)),
        // "twenty five" style — a tens word followed by a units word.
        [tens, units] => {
            let t = ten_en(tens)?;
            let u = unit_en(units)?;
            if u < 10 {
                Some(t + u)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_words_de(text: &str) -> Option<u32> {
    // German writes 21 as "einundzwanzig" (one-and-twenty), usually one word.
    let joined: String = text.split_whitespace().collect();
    Some(match joined.as_str() {
        "null" => 0,
        "eins" | "ein" | "eine" => 1,
        "zwei" => 2,
        "drei" => 3,
        "vier" => 4,
        "fünf" | "funf" => 5,
        "sechs" => 6,
        "sieben" => 7,
        "acht" => 8,
        "neun" => 9,
        "zehn" => 10,
        "elf" => 11,
        "zwölf" | "zwolf" => 12,
        "zwanzig" => 20,
        "dreißig" | "dreissig" => 30,
        "vierzig" => 40,
        "fünfzig" | "funfzig" => 50,
        "sechzig" => 60,
        "siebzig" => 70,
        "achtzig" => 80,
        "neunzig" => 90,
        "hundert" | "einhundert" => 100,
        other => return parse_de_compound(other),
    })
}

/// Handle German "<unit>und<tens>" compounds like "zweiundvierzig" = 42.
fn parse_de_compound(word: &str) -> Option<u32> {
    let (unit_part, tens_part) = word.split_once("und")?;
    let unit = match unit_part {
        "ein" => 1,
        "zwei" => 2,
        "drei" => 3,
        "vier" => 4,
        "fünf" | "funf" => 5,
        "sechs" => 6,
        "sieben" => 7,
        "acht" => 8,
        "neun" => 9,
        _ => return None,
    };
    let tens = match tens_part {
        "zwanzig" => 20,
        "dreißig" | "dreissig" => 30,
        "vierzig" => 40,
        "fünfzig" | "funfzig" => 50,
        "sechzig" => 60,
        "siebzig" => 70,
        "achtzig" => 80,
        "neunzig" => 90,
        _ => return None,
    };
    Some(tens + unit)
}

fn unit_tr(word: &str) -> Option<u32> {
    Some(match word {
        "sıfır" | "sifir" => 0,
        "bir" => 1,
        "iki" => 2,
        "üç" | "uc" => 3,
        "dört" | "dort" => 4,
        "beş" | "bes" => 5,
        "altı" | "alti" => 6,
        "yedi" => 7,
        "sekiz" => 8,
        "dokuz" => 9,
        _ => return None,
    })
}

fn ten_tr(word: &str) -> Option<u32> {
    Some(match word {
        "on" => 10,
        "yirmi" => 20,
        "otuz" => 30,
        "kırk" | "kirk" => 40,
        "elli" => 50,
        "altmış" | "altmis" => 60,
        "yetmiş" | "yetmis" => 70,
        "seksen" => 80,
        "doksan" => 90,
        _ => return None,
    })
}

fn parse_words_tr(text: &str) -> Option<u32> {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        ["yüz"] | ["yuz"] => Some(100),
        [w] => ten_tr(w).or_else(|| unit_tr(w)),
        // Turkish is fully analytic: "kırk iki" = forty two = 42.
        [tens, units] => {
            let t = ten_tr(tens)?;
            let u = unit_tr(units)?;
            if u < 10 {
                Some(t + u)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_language_independent() {
        assert_eq!(parse_number("42", Lang::En), Some(42));
        assert_eq!(parse_number("42", Lang::De), Some(42));
        assert_eq!(parse_number("0", Lang::Tr), Some(0));
        assert_eq!(parse_number("100", Lang::En), Some(100));
    }

    #[test]
    fn english_units_and_tens() {
        assert_eq!(parse_number("five", Lang::En), Some(5));
        assert_eq!(parse_number("fifty", Lang::En), Some(50));
        assert_eq!(parse_number("nineteen", Lang::En), Some(19));
        assert_eq!(parse_number("one hundred", Lang::En), Some(100));
    }

    #[test]
    fn english_compound_tens() {
        assert_eq!(parse_number("twenty five", Lang::En), Some(25));
        assert_eq!(parse_number("forty two", Lang::En), Some(42));
        assert_eq!(parse_number("ninety nine", Lang::En), Some(99));
    }

    #[test]
    fn english_rejects_nonsense() {
        assert_eq!(parse_number("fifty fifty", Lang::En), None);
        assert_eq!(parse_number("banana", Lang::En), None);
        assert_eq!(parse_number("", Lang::En), None);
    }

    #[test]
    fn german_words_and_compounds() {
        assert_eq!(parse_number("fünfzig", Lang::De), Some(50));
        assert_eq!(parse_number("funfzig", Lang::De), Some(50));
        assert_eq!(parse_number("zweiundvierzig", Lang::De), Some(42));
        assert_eq!(parse_number("hundert", Lang::De), Some(100));
        assert_eq!(parse_number("xyz", Lang::De), None);
    }

    #[test]
    fn turkish_words_and_compounds() {
        assert_eq!(parse_number("elli", Lang::Tr), Some(50));
        assert_eq!(parse_number("kırk iki", Lang::Tr), Some(42));
        assert_eq!(parse_number("bes", Lang::Tr), Some(5));
        assert_eq!(parse_number("yüz", Lang::Tr), Some(100));
        assert_eq!(parse_number("zzz", Lang::Tr), None);
    }
}
