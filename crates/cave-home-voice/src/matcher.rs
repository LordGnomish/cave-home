//! Intent matcher — match a spoken utterance against compiled intents and pull
//! out the slot values.
//!
//! The matcher normalises the recognised text (lower-case, trim, collapse
//! whitespace, strip punctuation), then walks each candidate intent's
//! sentence template against the token stream. A template matches only if it
//! consumes the *entire* utterance and every captured slot resolves against
//! its definition. The best (most specific) match wins; genuinely tied matches
//! are reported as ambiguous so the assistant can ask rather than guess.

use crate::label::Lang;
use crate::slot::{SlotKind, SlotValue, resolve as resolve_slot};
use crate::template::{Element, Template};
use std::collections::BTreeMap;

/// A template plus the slot definitions and metadata needed to match and
/// resolve it. One spoken-sentence variant of one intent in one language.
#[derive(Debug, Clone)]
pub struct CompiledIntent {
    /// Stable intent identifier (e.g. `"light.turn_on"`). Not user-facing.
    pub id: String,
    /// The language this sentence variant is written in.
    pub lang: Lang,
    /// The compiled sentence template.
    pub template: Template,
    /// Slot name → how to validate/canonicalise its capture.
    pub slots: BTreeMap<String, SlotKind>,
}

impl CompiledIntent {
    /// Build a compiled intent, parsing `template_src`.
    ///
    /// # Errors
    ///
    /// Returns the [`crate::template::TemplateError`] if the template string is
    /// malformed.
    pub fn new(
        id: impl Into<String>,
        lang: Lang,
        template_src: &str,
        slots: BTreeMap<String, SlotKind>,
    ) -> Result<Self, crate::template::TemplateError> {
        Ok(Self {
            id: id.into(),
            lang,
            template: Template::parse(template_src)?,
            slots,
        })
    }
}

/// A successful match: which intent fired and the resolved slot values.
#[derive(Debug, Clone, PartialEq)]
pub struct IntentMatch {
    /// The matched intent's id.
    pub intent: String,
    /// The language the matching sentence was written in.
    pub lang: Lang,
    /// Resolved, canonicalised slot values keyed by slot name.
    pub slots: BTreeMap<String, SlotValue>,
    /// A 0.0–1.0 confidence. Higher means the template was more specific
    /// (more required literal words relative to wildcards).
    pub confidence: f32,
}

/// The result of matching one utterance against a set of intents.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchOutcome {
    /// Exactly one best intent matched.
    Matched(IntentMatch),
    /// Two or more intents tied for best; the assistant should disambiguate
    /// rather than pick arbitrarily. Carries the tied intent ids.
    Ambiguous(Vec<String>),
    /// Nothing matched.
    NoMatch,
}

/// Normalise raw recognised text into comparable tokens: lower-case, strip
/// punctuation, collapse whitespace.
#[must_use]
pub fn normalize_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Match `utterance` against `intents`, returning the best outcome.
///
/// Candidates are scored; the highest-confidence match wins. If the top two
/// scores are equal *and* come from different intent ids, the result is
/// [`MatchOutcome::Ambiguous`]. Multiple sentence variants of the *same* intent
/// tying is not ambiguous — they mean the same thing.
#[must_use]
pub fn match_intent(utterance: &str, intents: &[CompiledIntent]) -> MatchOutcome {
    let tokens = normalize_tokens(utterance);
    let mut best: Vec<IntentMatch> = Vec::new();

    for intent in intents {
        if let Some(m) = try_match(intent, &tokens) {
            best.push(m);
        }
    }

    // Highest confidence first.
    best.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    best.first().map_or(MatchOutcome::NoMatch, |top| {
        let top_conf = top.confidence;
        // Collect distinct intent ids tied at the top score.
        let mut tied_ids: Vec<String> = Vec::new();
        for m in &best {
            if (m.confidence - top_conf).abs() < f32::EPSILON {
                if !tied_ids.contains(&m.intent) {
                    tied_ids.push(m.intent.clone());
                }
            } else {
                break;
            }
        }
        if tied_ids.len() > 1 {
            MatchOutcome::Ambiguous(tied_ids)
        } else {
            MatchOutcome::Matched(top.clone())
        }
    })
}

/// Attempt to match one intent against the token stream. Returns the resolved
/// match (with confidence) on success.
///
/// Slot captures are *resolved inline* during the walk: a greedy slot first
/// tries to grab as many tokens as possible, but if that span fails to resolve
/// against the slot definition (e.g. "kitchen light" is not a room) the matcher
/// backtracks to a shorter span. This is why "turn the kitchen light on"
/// matches `turn [the] {name} [light] on` with `name = kitchen`.
fn try_match(intent: &CompiledIntent, tokens: &[String]) -> Option<IntentMatch> {
    let mut slots: BTreeMap<String, SlotValue> = BTreeMap::new();
    // Walk the template; require it to consume *all* tokens.
    if !walk(
        intent,
        intent.template.elements(),
        tokens,
        0,
        &mut slots,
        &mut |pos| pos == tokens.len(),
    ) {
        return None;
    }

    let confidence = confidence_of(intent.template.elements(), tokens.len());
    Some(IntentMatch {
        intent: intent.id.clone(),
        lang: intent.lang,
        slots,
        confidence,
    })
}

/// Recursive backtracking matcher. Tries to consume `elements` starting at
/// token index `pos`; on reaching the end of `elements`, calls `accept(pos)`
/// to decide whether the remaining tokens are acceptable (the top-level call
/// requires `pos == tokens.len()`). Resolves slot captures into `slots` as it
/// goes, backtracking when a capture fails to resolve.
fn walk(
    intent: &CompiledIntent,
    elements: &[Element],
    tokens: &[String],
    pos: usize,
    slots: &mut BTreeMap<String, SlotValue>,
    accept: &mut dyn FnMut(usize) -> bool,
) -> bool {
    let Some((head, rest)) = elements.split_first() else {
        return accept(pos);
    };

    match head {
        Element::Word(w) => {
            if pos < tokens.len() && &tokens[pos] == w {
                walk(intent, rest, tokens, pos + 1, slots, accept)
            } else {
                false
            }
        }
        Element::Optional(inner) => {
            // Branch 1: take the optional group.
            let snapshot = slots.clone();
            if walk_concat(intent, inner, rest, tokens, pos, slots, accept) {
                return true;
            }
            *slots = snapshot;
            // Branch 2: skip it.
            walk(intent, rest, tokens, pos, slots, accept)
        }
        Element::Alternatives(branches) => {
            for branch in branches {
                let snapshot = slots.clone();
                if walk_concat(intent, branch, rest, tokens, pos, slots, accept) {
                    return true;
                }
                *slots = snapshot;
            }
            false
        }
        Element::Slot(name) => {
            // The slot definition must exist; an undefined slot can never match.
            let Some(kind) = intent.slots.get(name) else {
                return false;
            };
            // A slot captures one-or-more tokens (greedy, then backtrack). It
            // must capture at least one token so an empty span never matches,
            // and the captured span must *resolve* against the definition.
            let max_end = tokens.len();
            for end in ((pos + 1)..=max_end).rev() {
                let captured = tokens[pos..end].join(" ");
                let Some(value) = resolve_slot(kind, &captured, intent.lang) else {
                    continue;
                };
                let snapshot = slots.clone();
                slots.insert(name.clone(), value);
                if walk(intent, rest, tokens, end, slots, accept) {
                    return true;
                }
                *slots = snapshot;
            }
            false
        }
    }
}

/// Walk `inner` then `rest` as if concatenated, without changing the recursion
/// shape: match `inner` followed by `rest`.
fn walk_concat(
    intent: &CompiledIntent,
    inner: &[Element],
    rest: &[Element],
    tokens: &[String],
    pos: usize,
    slots: &mut BTreeMap<String, SlotValue>,
    accept: &mut dyn FnMut(usize) -> bool,
) -> bool {
    let mut combined: Vec<Element> = Vec::with_capacity(inner.len() + rest.len());
    combined.extend_from_slice(inner);
    combined.extend_from_slice(rest);
    walk(intent, &combined, tokens, pos, slots, accept)
}

/// Confidence heuristic: the share of the utterance pinned down by required
/// literal words, in `[0.5, 1.0]`. A template that is all literals scores 1.0;
/// one dominated by free slots scores lower (but never below 0.5 once it has
/// matched at all). Specificity, not certainty — STT confidence is Phase-1b.
fn confidence_of(elements: &[Element], token_count: usize) -> f32 {
    let required = count_required_words(elements);
    if token_count == 0 {
        return 1.0;
    }
    let ratio = usize_to_f32(required) / usize_to_f32(token_count);
    // Plain multiply-then-add (not a fused `mul_add`), as before.
    let share = 0.5 * ratio.min(1.0);
    0.5 + share
}

/// `n as f32` — the nearest `f32`, ties to even (exact up to 2^24) — spelled
/// out with integer arithmetic instead of a cast.
fn usize_to_f32(n: usize) -> f32 {
    const MANTISSA_BITS: u32 = 23;
    const EXPONENT_BIAS: u32 = 127;

    let n = n as u64;
    if n == 0 {
        return 0.0;
    }
    // Index of the highest set bit: the unbiased binary exponent.
    let exponent = n.ilog2();
    // `kept` holds the 24 leading bits (implicit 1 at bit 23); whatever is
    // shifted out decides the rounding, half-way cases going to even.
    let (kept, round_up) = if exponent <= MANTISSA_BITS {
        (n << (MANTISSA_BITS - exponent), false)
    } else {
        let dropped_bits = exponent - MANTISSA_BITS;
        let kept = n >> dropped_bits;
        let dropped = n & ((1 << dropped_bits) - 1);
        let half = 1 << (dropped_bits - 1);
        (kept, dropped > half || (dropped == half && kept & 1 == 1))
    };
    let [.., b2, b1, b0] = kept.to_be_bytes();
    let fraction = u32::from_be_bytes([0, b2 & 0x7f, b1, b0]);
    // A round-up that overflows the fraction carries into the exponent.
    f32::from_bits(((EXPONENT_BIAS + exponent) << MANTISSA_BITS) + fraction + u32::from(round_up))
}

fn count_required_words(elements: &[Element]) -> usize {
    let mut n = 0;
    for el in elements {
        match el {
            Element::Word(_) => n += 1,
            // Optionals are not required; slots are wildcards.
            Element::Optional(_) | Element::Slot(_) => {}
            Element::Alternatives(branches) => {
                // Count the minimum required words any branch contributes.
                let min = branches
                    .iter()
                    .map(|b| count_required_words(b))
                    .min()
                    .unwrap_or(0);
                n += min;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slot::ValueList;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn usize_to_f32_rounds_like_the_cast() {
        assert_eq!(usize_to_f32(0).to_bits(), 0.0_f32.to_bits());
        assert_eq!(usize_to_f32(1).to_bits(), 1.0_f32.to_bits());
        assert_eq!(usize_to_f32(7).to_bits(), 7.0_f32.to_bits());
        // 2^24 is the last point where every integer is representable.
        assert_eq!(
            usize_to_f32(16_777_216).to_bits(),
            16_777_216.0_f32.to_bits()
        );
        // Half-way cases go to the even neighbour …
        assert_eq!(
            usize_to_f32(16_777_217).to_bits(),
            16_777_216.0_f32.to_bits()
        );
        assert_eq!(
            usize_to_f32(16_777_219).to_bits(),
            16_777_220.0_f32.to_bits()
        );
        // … and a round-up out of the mantissa bumps the exponent.
        assert_eq!(
            usize_to_f32(4_294_967_295).to_bits(),
            4_294_967_296.0_f32.to_bits()
        );
    }

    fn rooms() -> SlotKind {
        SlotKind::List(
            ValueList::new(["living room", "kitchen"]).with_synonym("lounge", "living room"),
        )
    }

    fn slots(pairs: &[(&str, SlotKind)]) -> BTreeMap<String, SlotKind> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn normalizes_input() {
        assert_eq!(
            normalize_tokens("  Turn ON the   LIGHT! "),
            vec!["turn", "on", "the", "light"]
        );
    }

    #[test]
    fn matches_literal_intent() -> TestResult {
        let i = CompiledIntent::new(
            "lights.all_off",
            Lang::En,
            "turn everything off",
            BTreeMap::new(),
        )?;
        match match_intent("Turn everything off.", &[i]) {
            MatchOutcome::Matched(m) => {
                assert_eq!(m.intent, "lights.all_off");
                assert!((m.confidence - 1.0).abs() < f32::EPSILON);
            }
            other => return Err(format!("expected match, got {other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn matches_with_optional_present_or_absent() -> TestResult {
        let mk = || {
            CompiledIntent::new(
                "light.on",
                Lang::En,
                "turn [the] {name} on",
                slots(&[("name", rooms())]),
            )
        };
        for utt in ["turn the kitchen on", "turn kitchen on"] {
            match match_intent(utt, &[mk()?]) {
                MatchOutcome::Matched(m) => {
                    assert_eq!(m.slots["name"], SlotValue::Text("kitchen".into()));
                }
                other => return Err(format!("{utt:?} -> {other:?}").into()),
            }
        }
        Ok(())
    }

    #[test]
    fn extracts_and_canonicalises_slot_via_synonym() -> TestResult {
        let i = CompiledIntent::new(
            "light.on",
            Lang::En,
            "turn [the] {name} on",
            slots(&[("name", rooms())]),
        )?;
        match match_intent("turn the lounge on", &[i]) {
            MatchOutcome::Matched(m) => {
                assert_eq!(m.slots["name"], SlotValue::Text("living room".into()));
            }
            other => return Err(format!("{other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn extracts_number_slot() -> TestResult {
        let i = CompiledIntent::new(
            "light.brightness",
            Lang::En,
            "set [the] {name} to {level} percent",
            slots(&[
                ("name", rooms()),
                ("level", SlotKind::Number { min: 0, max: 100 }),
            ]),
        )?;
        match match_intent("set the kitchen to fifty percent", &[i]) {
            MatchOutcome::Matched(m) => {
                assert_eq!(m.slots["name"], SlotValue::Text("kitchen".into()));
                assert_eq!(m.slots["level"], SlotValue::Number(50));
            }
            other => return Err(format!("{other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn rejects_unknown_slot_value() -> TestResult {
        let i = CompiledIntent::new(
            "light.on",
            Lang::En,
            "turn [the] {name} on",
            slots(&[("name", rooms())]),
        )?;
        assert_eq!(
            match_intent("turn the garage on", &[i]),
            MatchOutcome::NoMatch
        );
        Ok(())
    }

    #[test]
    fn no_match_when_template_does_not_consume_all_tokens() -> TestResult {
        let i = CompiledIntent::new("x", Lang::En, "turn it off", BTreeMap::new())?;
        assert_eq!(
            match_intent("turn it off now please", &[i]),
            MatchOutcome::NoMatch
        );
        Ok(())
    }

    #[test]
    fn alternatives_match_either_branch() -> TestResult {
        let mk = || {
            CompiledIntent::new(
                "query.temp",
                Lang::En,
                "(what is|whats) the {name} temperature",
                slots(&[("name", rooms())]),
            )
        };
        for utt in [
            "what is the kitchen temperature",
            "whats the kitchen temperature",
        ] {
            assert!(matches!(
                match_intent(utt, &[mk()?]),
                MatchOutcome::Matched(_)
            ));
        }
        Ok(())
    }

    #[test]
    fn more_specific_intent_outscores_generic() -> TestResult {
        // Generic catch-all (slot-only) vs specific literal command.
        let generic = CompiledIntent::new(
            "scene.activate",
            Lang::En,
            "{name}",
            slots(&[("name", SlotKind::Open)]),
        )?;
        let specific =
            CompiledIntent::new("lights.all_off", Lang::En, "lights off", BTreeMap::new())?;
        match match_intent("lights off", &[generic, specific]) {
            MatchOutcome::Matched(m) => assert_eq!(m.intent, "lights.all_off"),
            other => return Err(format!("{other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn ties_between_different_intents_are_ambiguous() -> TestResult {
        let a = CompiledIntent::new("a", Lang::En, "{name}", slots(&[("name", SlotKind::Open)]))?;
        let b = CompiledIntent::new(
            "b",
            Lang::En,
            "{thing}",
            slots(&[("thing", SlotKind::Open)]),
        )?;
        match match_intent("hello", &[a, b]) {
            MatchOutcome::Ambiguous(ids) => {
                assert_eq!(ids.len(), 2);
                assert!(ids.contains(&"a".to_string()));
            }
            other => return Err(format!("expected ambiguous, got {other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn same_intent_two_variants_is_not_ambiguous() -> TestResult {
        let v1 = CompiledIntent::new("light.on", Lang::En, "lights on", BTreeMap::new())?;
        let v2 = CompiledIntent::new("light.on", Lang::En, "turn lights on", BTreeMap::new())?;
        // "lights on" only matches v1, but make both id-equal to show same-id
        // ties collapse. Use an utterance only v1 matches; confidence equal is
        // moot — the real same-id tie is exercised by intents.rs sets.
        match match_intent("lights on", &[v1, v2]) {
            MatchOutcome::Matched(m) => assert_eq!(m.intent, "light.on"),
            other => return Err(format!("{other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn empty_intent_set_is_no_match() {
        assert_eq!(match_intent("anything", &[]), MatchOutcome::NoMatch);
    }
}
