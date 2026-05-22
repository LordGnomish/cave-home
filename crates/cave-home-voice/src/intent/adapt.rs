// SPDX-License-Identifier: Apache-2.0
//! Adapt intent parser — keyword + regex matchers.
//!
//! # Upstream:
//! - `MycroftAI/adapt@d3c2c2f:adapt/intent.py::IntentBuilder` — the
//!   builder pattern for defining intents (required / optional / one_of
//!   keyword lists).
//! - `MycroftAI/adapt@d3c2c2f:adapt/engine.py::IntentDeterminationEngine.determine_intent` —
//!   the resolver that walks intents and computes confidence as
//!   `matched_required / total_required`. Reproduced one-to-one in
//!   [`AdaptParser::parse`].
//! - `MycroftAI/adapt@d3c2c2f:adapt/expander.py::Expander` — slot value
//!   extraction; we reproduce a simplified linear scan.

use std::collections::HashMap;

use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::{IntentMatch, IntentParser};

/// One registered intent.
///
/// # Upstream:
/// `MycroftAI/adapt@d3c2c2f:adapt/intent.py::Intent` — `name` +
/// `required` + `optional` + `at_least_one`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptIntent {
    /// Dotted intent name (e.g. `lighting.turn_on`).
    pub name: String,
    /// Languages this intent applies to (e.g. `["tr", "en"]`).
    /// Empty ⇒ matches any language.
    #[serde(default)]
    pub languages: Vec<String>,
    /// Every term must appear in the utterance.
    pub required: Vec<Vec<String>>,
    /// At least one term per group must appear.
    #[serde(default)]
    pub at_least_one: Vec<Vec<String>>,
    /// Optional terms — captured into slots when present.
    /// Each entry is `(slot_name, alternatives)`.
    #[serde(default)]
    pub optional_slots: Vec<(String, Vec<String>)>,
    /// Regex slot extractors. Each entry is `(slot_name, pattern)`;
    /// when the regex matches, the first capture group is the slot
    /// value.
    #[serde(default)]
    pub regex_slots: Vec<(String, String)>,
}

impl AdaptIntent {
    /// Builder convenience.
    #[must_use]
    pub fn new<S: Into<String>>(name: S) -> Self {
        Self {
            name: name.into(),
            languages: Vec::new(),
            required: Vec::new(),
            at_least_one: Vec::new(),
            optional_slots: Vec::new(),
            regex_slots: Vec::new(),
        }
    }

    /// Add a required term group — at least one of these must appear.
    /// Multiple `required` calls each create a new group.
    #[must_use]
    pub fn require<I, S>(mut self, alternatives: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.required
            .push(alternatives.into_iter().map(Into::into).collect());
        self
    }

    /// Add an optional slot.
    #[must_use]
    pub fn optional<S, I, T>(mut self, slot: S, alternatives: I) -> Self
    where
        S: Into<String>,
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        self.optional_slots.push((
            slot.into(),
            alternatives.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Add a regex slot extractor.
    #[must_use]
    pub fn regex_slot<S: Into<String>, P: Into<String>>(mut self, slot: S, pattern: P) -> Self {
        self.regex_slots.push((slot.into(), pattern.into()));
        self
    }

    /// Limit the intent to specific languages.
    #[must_use]
    pub fn for_language<S: Into<String>>(mut self, lang: S) -> Self {
        self.languages.push(lang.into());
        self
    }
}

/// Adapt parser instance.
///
/// # Upstream:
/// `MycroftAI/adapt@d3c2c2f:adapt/engine.py::IntentDeterminationEngine`
#[derive(Default)]
pub struct AdaptParser {
    intents: Mutex<Vec<AdaptIntent>>,
    /// Cache of compiled regex per regex_slot entry — keyed by intent
    /// name + slot name to avoid recompiling per parse call.
    regex_cache: Mutex<HashMap<String, Regex>>,
}

impl AdaptParser {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an intent (parity with `engine.register_intent`).
    pub fn register(&self, intent: AdaptIntent) {
        // Eagerly compile any regex slots so the parse path stays cheap.
        for (slot, pattern) in &intent.regex_slots {
            let key = format!("{}::{}", intent.name, slot);
            if let Ok(re) = Regex::new(pattern) {
                self.regex_cache.lock().insert(key, re);
            }
        }
        self.intents.lock().push(intent);
    }

    /// Number of registered intents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.intents.lock().len()
    }

    /// True iff no intents are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.intents.lock().is_empty()
    }
}

impl IntentParser for AdaptParser {
    fn name(&self) -> &'static str {
        "adapt"
    }

    fn parse(&self, utterance: &str, language: &str) -> Option<IntentMatch> {
        let normalised = utterance.to_lowercase();
        let intents = self.intents.lock().clone();
        let mut best: Option<IntentMatch> = None;
        for intent in &intents {
            if !intent.languages.is_empty()
                && !intent.languages.iter().any(|l| l == language)
            {
                continue;
            }
            // Each required group must contribute at least one term.
            let mut required_hits = 0_usize;
            for group in &intent.required {
                let hit = group.iter().any(|term| normalised.contains(&term.to_lowercase()));
                if !hit {
                    required_hits = 0;
                    break;
                }
                required_hits += 1;
            }
            if intent.required.is_empty() || required_hits == intent.required.len() {
                let total_required = intent.required.len().max(1) as f32;
                let mut score = required_hits.max(intent.required.len()) as f32 / total_required;
                // at_least_one bonus
                for group in &intent.at_least_one {
                    if group
                        .iter()
                        .any(|term| normalised.contains(&term.to_lowercase()))
                    {
                        score = (score + 0.1).min(1.0);
                    } else if !group.is_empty() {
                        // At-least-one not satisfied — disqualify.
                        score = 0.0;
                        break;
                    }
                }
                if score <= 0.0 {
                    continue;
                }
                let mut slots = HashMap::new();
                for (slot, alts) in &intent.optional_slots {
                    for alt in alts {
                        if normalised.contains(&alt.to_lowercase()) {
                            slots.insert(slot.clone(), alt.clone());
                            break;
                        }
                    }
                }
                // Regex slots (Adapt's Expander).
                let cache = self.regex_cache.lock();
                for (slot, _pattern) in &intent.regex_slots {
                    let key = format!("{}::{}", intent.name, slot);
                    if let Some(re) = cache.get(&key) {
                        if let Some(caps) = re.captures(utterance) {
                            if let Some(group1) = caps.get(1) {
                                slots.insert(slot.clone(), group1.as_str().to_string());
                            }
                        }
                    }
                }
                drop(cache);
                let candidate = IntentMatch {
                    name: intent.name.clone(),
                    source: "adapt".into(),
                    confidence: score,
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
        }
        best
    }
}
