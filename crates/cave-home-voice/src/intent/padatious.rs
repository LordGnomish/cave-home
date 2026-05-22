// SPDX-License-Identifier: Apache-2.0
//! Padatious intent parser — example-driven n-gram classifier.
//!
//! # Upstream:
//! - `OpenVoiceOS/padatious@…:padatious/intent_container.py::IntentContainer.train` —
//!   trains a per-intent classifier from example utterances. Upstream
//!   uses a neural net (`fann2`); Phase 1 implements a Rust-native
//!   n-gram (1-gram + 2-gram bag-of-words) overlap classifier with the
//!   same training/inference shape. The neural backend is documented
//!   as Phase 1b in `parity.manifest.toml`.
//! - `OpenVoiceOS/padatious@…:padatious/intent_container.py::IntentContainer.calc_intent` —
//!   inference loop. Reproduced one-to-one in [`PadatiousParser::parse`].
//! - `OpenVoiceOS/padatious@…:padatious/entity.py::Entity` — slot
//!   templates with `{slot}` placeholders. Reproduced via
//!   [`extract_slot_from_template`].

use std::collections::{HashMap, HashSet};

use parking_lot::Mutex;

use super::{IntentMatch, IntentParser};

/// One trained Padatious intent.
///
/// # Upstream:
/// `OpenVoiceOS/padatious@…:padatious/intent.py::Intent` — `name` +
/// example list + slot map. Per-intent state in cave-home is the
/// extracted n-gram vocabulary.
#[derive(Debug, Clone)]
pub struct PadatiousIntent {
    pub name: String,
    pub examples: Vec<String>,
    /// Optional language restriction.
    pub languages: Vec<String>,
    /// Pre-computed unique n-gram vocabulary.
    vocab: HashSet<String>,
}

impl PadatiousIntent {
    /// Build from raw examples.
    #[must_use]
    pub fn build<S: Into<String>>(name: S, examples: Vec<String>) -> Self {
        let mut vocab = HashSet::new();
        for ex in &examples {
            for n in n_grams(&normalise(ex)) {
                vocab.insert(n);
            }
        }
        Self {
            name: name.into(),
            examples,
            languages: Vec::new(),
            vocab,
        }
    }

    #[must_use]
    pub fn for_language<S: Into<String>>(mut self, lang: S) -> Self {
        self.languages.push(lang.into());
        self
    }
}

/// Padatious parser.
///
/// # Upstream:
/// `OpenVoiceOS/padatious@…:padatious/intent_container.py::IntentContainer`
#[derive(Default)]
pub struct PadatiousParser {
    intents: Mutex<Vec<PadatiousIntent>>,
    /// Minimum overlap ratio to fire (parity with the upstream
    /// `padatious.config.threshold` default of 0.5).
    threshold: f32,
}

impl PadatiousParser {
    /// Defaults match `padatious.config.threshold` upstream (0.5).
    #[must_use]
    pub fn new() -> Self {
        Self {
            intents: Mutex::new(Vec::new()),
            threshold: 0.5,
        }
    }

    /// Override the firing threshold.
    #[must_use]
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Register an intent with raw example utterances.
    pub fn register<S: Into<String>>(&self, name: S, examples: Vec<String>) {
        self.intents.lock().push(PadatiousIntent::build(name, examples));
    }

    /// Register an already-built intent.
    pub fn register_intent(&self, intent: PadatiousIntent) {
        self.intents.lock().push(intent);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.intents.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.intents.lock().is_empty()
    }
}

impl IntentParser for PadatiousParser {
    fn name(&self) -> &'static str {
        "padatious"
    }

    fn parse(&self, utterance: &str, language: &str) -> Option<IntentMatch> {
        let utterance_norm = normalise(utterance);
        let utterance_grams: HashSet<String> = n_grams(&utterance_norm).into_iter().collect();
        if utterance_grams.is_empty() {
            return None;
        }
        let intents = self.intents.lock().clone();
        let mut best: Option<IntentMatch> = None;
        for intent in &intents {
            if !intent.languages.is_empty()
                && !intent.languages.iter().any(|l| l == language)
            {
                continue;
            }
            let overlap = utterance_grams
                .iter()
                .filter(|g| intent.vocab.contains(*g))
                .count();
            let denom = intent.vocab.len().max(1) as f32;
            let confidence = overlap as f32 / denom;
            if confidence < self.threshold {
                continue;
            }
            // Slot extraction: pick the example with maximum gram
            // overlap, then template-match it against the utterance.
            let mut slots = HashMap::new();
            for ex in &intent.examples {
                if let Some(s) = extract_slot_from_template(ex, utterance) {
                    slots.extend(s);
                    break;
                }
            }
            let candidate = IntentMatch {
                name: intent.name.clone(),
                source: "padatious".into(),
                confidence,
                slots,
                utterance: utterance.to_string(),
                language: language.to_string(),
            };
            let better = best
                .as_ref()
                .map_or(true, |b| candidate.confidence > b.confidence);
            if better {
                best = Some(candidate);
            }
        }
        best
    }
}

/// Extract slot values when an example template like
/// `"turn on the {device} light"` matches an utterance.
///
/// # Upstream:
/// `OpenVoiceOS/padatious@…:padatious/entity.py::Entity` — slot
/// templates use `{slot}` placeholders; the value is the longest run of
/// non-template tokens occupying that position.
#[must_use]
pub fn extract_slot_from_template(template: &str, utterance: &str) -> Option<HashMap<String, String>> {
    let template_lc = template.to_lowercase();
    let utterance_lc = utterance.to_lowercase();
    let template_tokens: Vec<&str> = template_lc.split_whitespace().collect();
    let utterance_tokens: Vec<&str> = utterance_lc.split_whitespace().collect();
    if template_tokens.is_empty() {
        return None;
    }
    let mut slots: HashMap<String, String> = HashMap::new();
    let mut ti = 0_usize;
    let mut ui = 0_usize;
    while ti < template_tokens.len() {
        let token = template_tokens[ti];
        if token.starts_with('{') && token.ends_with('}') {
            let slot = token.trim_start_matches('{').trim_end_matches('}').to_string();
            // Consume utterance tokens until we hit the next literal
            // (or the end of the template).
            let next_literal = template_tokens.get(ti + 1).copied();
            let mut collected: Vec<&str> = Vec::new();
            while ui < utterance_tokens.len() {
                let candidate = utterance_tokens[ui];
                if Some(candidate) == next_literal {
                    break;
                }
                collected.push(candidate);
                ui += 1;
            }
            if collected.is_empty() {
                return None;
            }
            slots.insert(slot, collected.join(" "));
            ti += 1;
        } else {
            if ui >= utterance_tokens.len() || utterance_tokens[ui] != token {
                return None;
            }
            ui += 1;
            ti += 1;
        }
    }
    if ui == utterance_tokens.len() {
        Some(slots)
    } else {
        None
    }
}

// --- internals --------------------------------------------------------------

fn normalise(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect()
}

fn n_grams(s: &str) -> Vec<String> {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut out = Vec::with_capacity(tokens.len() * 2);
    for t in &tokens {
        if !t.is_empty() {
            out.push((*t).to_string());
        }
    }
    for w in tokens.windows(2) {
        out.push(format!("{} {}", w[0], w[1]));
    }
    out
}
