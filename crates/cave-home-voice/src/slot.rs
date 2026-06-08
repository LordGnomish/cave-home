//! Slot definitions — what a `{placeholder}` is allowed to capture.
//!
//! A template like `"turn [the] {name} on"` has a `name` slot. A *slot
//! definition* says what `name` may be: a fixed list of device or room names
//! (with synonyms — "lounge" really means "living room"), an open free-text
//! capture, or a number (brightness, temperature).
//!
//! Resolving a captured phrase against its slot definition does two jobs:
//! 1. **Validation** — reject a value that is not allowed (an unknown room).
//! 2. **Canonicalisation** — fold a synonym onto its canonical value, so the
//!    rest of cave-home only ever sees `"living room"`, never `"lounge"`.

use crate::label::Lang;
use crate::number_words::parse_number;
use std::collections::BTreeMap;

/// The kind of value a slot accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotKind {
    /// A fixed vocabulary: every spoken phrase must resolve to one of the
    /// listed canonical values (possibly via a synonym). Used for device and
    /// room names — the engine must not invent a device that does not exist.
    List(ValueList),
    /// A bounded whole number, inclusive range. Spoken digits or number-words
    /// are both accepted (see [`crate::number_words`]).
    Number { min: u32, max: u32 },
    /// Free text — capture whatever was said, trimmed. Used for open queries
    /// (e.g. a custom scene name typed by the household).
    Open,
}

/// A fixed vocabulary of canonical values plus synonym → canonical mappings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValueList {
    /// canonical value → itself (kept so membership is O(log n) and order is
    /// stable for tests); synonyms map onto a canonical value.
    map: BTreeMap<String, String>,
}

impl ValueList {
    /// Build a value list from canonical values.
    #[must_use]
    pub fn new<I, S>(canonical: I) -> ValueList
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut map = BTreeMap::new();
        for c in canonical {
            let c = c.into();
            let key = normalize(&c);
            map.insert(key, c);
        }
        ValueList { map }
    }

    /// Add a synonym (`spoken`) that resolves to an existing canonical value.
    /// If the canonical value is not already present it is added too, so a
    /// list can be built fluently.
    #[must_use]
    pub fn with_synonym(mut self, spoken: &str, canonical: &str) -> ValueList {
        let canon_key = normalize(canonical);
        if !self.map.contains_key(&canon_key) {
            self.map.insert(canon_key, canonical.to_string());
        }
        self.map.insert(normalize(spoken), canonical.to_string());
        self
    }

    /// Resolve a spoken phrase to its canonical value, or [`None`] if it is not
    /// in this vocabulary.
    #[must_use]
    pub fn resolve(&self, spoken: &str) -> Option<String> {
        self.map.get(&normalize(spoken)).cloned()
    }

    /// The canonical values (deduplicated), sorted — handy for tests/UX.
    #[must_use]
    pub fn canonical_values(&self) -> Vec<String> {
        let mut v: Vec<String> = self.map.values().cloned().collect();
        v.sort();
        v.dedup();
        v
    }
}

/// The outcome of resolving a captured phrase against a slot definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotValue {
    /// A canonical text value (from a [`SlotKind::List`] or [`SlotKind::Open`]).
    Text(String),
    /// A parsed number (from a [`SlotKind::Number`]).
    Number(u32),
}

impl SlotValue {
    /// The value rendered as text (numbers become their decimal form). Used by
    /// the response generator.
    #[must_use]
    pub fn as_text(&self) -> String {
        match self {
            SlotValue::Text(t) => t.clone(),
            SlotValue::Number(n) => n.to_string(),
        }
    }

    /// The number, if this is a [`SlotValue::Number`].
    #[must_use]
    pub const fn as_number(&self) -> Option<u32> {
        match self {
            SlotValue::Number(n) => Some(*n),
            SlotValue::Text(_) => None,
        }
    }
}

/// Resolve a raw captured phrase against a slot kind in a given language.
///
/// Returns [`None`] when the value is not allowed (unknown list member, number
/// out of range or unparseable, empty open capture) — the caller treats that
/// as a non-match for the candidate intent.
#[must_use]
pub fn resolve(kind: &SlotKind, captured: &str, lang: Lang) -> Option<SlotValue> {
    let captured = captured.trim();
    if captured.is_empty() {
        return None;
    }
    match kind {
        SlotKind::List(list) => list.resolve(captured).map(SlotValue::Text),
        SlotKind::Number { min, max } => {
            let n = parse_number(captured, lang)?;
            if n >= *min && n <= *max {
                Some(SlotValue::Number(n))
            } else {
                None
            }
        }
        SlotKind::Open => Some(SlotValue::Text(captured.to_string())),
    }
}

/// Normalise a phrase for vocabulary lookup: lower-case, collapse whitespace,
/// drop punctuation. Mirrors the input normalisation in [`crate::matcher`] so a
/// spoken word matches a list entry regardless of casing/spacing.
fn normalize(s: &str) -> String {
    let mut out = String::new();
    let mut last_space = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            last_space = false;
        } else if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        }
        // punctuation dropped
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rooms() -> ValueList {
        ValueList::new(["living room", "kitchen", "bedroom"])
            .with_synonym("lounge", "living room")
            .with_synonym("sitting room", "living room")
    }

    #[test]
    fn list_resolves_canonical_and_normalises() {
        let r = rooms();
        assert_eq!(r.resolve("Living Room"), Some("living room".to_string()));
        assert_eq!(r.resolve("  kitchen  "), Some("kitchen".to_string()));
    }

    #[test]
    fn list_resolves_synonyms() {
        let r = rooms();
        assert_eq!(r.resolve("lounge"), Some("living room".to_string()));
        assert_eq!(r.resolve("sitting room"), Some("living room".to_string()));
    }

    #[test]
    fn list_rejects_unknown_value() {
        assert_eq!(rooms().resolve("garage"), None);
    }

    #[test]
    fn canonical_values_dedup() {
        assert_eq!(
            rooms().canonical_values(),
            vec![
                "bedroom".to_string(),
                "kitchen".to_string(),
                "living room".to_string()
            ]
        );
    }

    #[test]
    fn resolve_list_slot() {
        let kind = SlotKind::List(rooms());
        assert_eq!(
            resolve(&kind, "Lounge", Lang::En),
            Some(SlotValue::Text("living room".to_string()))
        );
        assert_eq!(resolve(&kind, "garage", Lang::En), None);
    }

    #[test]
    fn resolve_number_slot_in_range() {
        let kind = SlotKind::Number { min: 0, max: 100 };
        assert_eq!(resolve(&kind, "fifty", Lang::En), Some(SlotValue::Number(50)));
        assert_eq!(resolve(&kind, "75", Lang::En), Some(SlotValue::Number(75)));
    }

    #[test]
    fn resolve_number_slot_rejects_out_of_range_and_garbage() {
        let kind = SlotKind::Number { min: 0, max: 100 };
        assert_eq!(resolve(&kind, "one hundred fifty", Lang::En), None);
        assert_eq!(resolve(&kind, "banana", Lang::En), None);
    }

    #[test]
    fn resolve_open_slot_keeps_text() {
        assert_eq!(
            resolve(&SlotKind::Open, "  movie night  ", Lang::En),
            Some(SlotValue::Text("movie night".to_string()))
        );
        assert_eq!(resolve(&SlotKind::Open, "   ", Lang::En), None);
    }

    #[test]
    fn slot_value_renders_text_and_number() {
        assert_eq!(SlotValue::Number(42).as_text(), "42");
        assert_eq!(SlotValue::Number(42).as_number(), Some(42));
        assert_eq!(SlotValue::Text("x".into()).as_number(), None);
    }
}
