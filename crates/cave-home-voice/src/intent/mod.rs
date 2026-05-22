// SPDX-License-Identifier: Apache-2.0
//! Intent parsing — Adapt (keyword + regex) and Padatious (n-gram NLU).
//!
//! # Upstream:
//! - `MycroftAI/adapt@d3c2c2f:adapt/intent.py::IntentBuilder` — keyword
//!   matcher; ported to [`adapt::AdaptIntent`] +
//!   [`adapt::AdaptParser`].
//! - `OpenVoiceOS/padatious@…:padatious/intent_container.py` — n-gram
//!   classifier; cave-home Phase 1 implements a Rust-native n-gram
//!   bag-of-words classifier with the same intent envelope shape (see
//!   [`padatious::PadatiousParser`]). The full neural model is
//!   documented as Phase 1b in `parity.manifest.toml`.
//! - `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/intent_services/__init__.py::IntentService.handle_utterance` —
//!   the cascade that tries Padatious first, then Adapt, then
//!   fallback. Reproduced in [`IntentRouter::resolve`].

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub mod adapt;
pub mod padatious;
pub mod parser;

pub use adapt::{AdaptIntent, AdaptParser};
pub use padatious::{PadatiousIntent, PadatiousParser};
pub use parser::IntentParser;

/// A resolved intent — the routing record handed to the skill layer.
///
/// # Upstream:
/// `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/intent_services/__init__.py::IntentMatch`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentMatch {
    /// Dotted intent name (e.g. `lighting.turn_on`).
    pub name: String,
    /// Parser that produced the match (`adapt` / `padatious`).
    pub source: String,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
    /// Slot values extracted from the utterance.
    #[serde(default)]
    pub slots: HashMap<String, String>,
    /// Original utterance that triggered the match.
    pub utterance: String,
    /// Language tag of the utterance.
    pub language: String,
}

/// Multi-parser router.
///
/// # Upstream:
/// `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/intent_services/__init__.py::IntentService.handle_utterance`
/// — cascade order: padatious → adapt → fallback. The first parser to
/// return a match above its confidence floor wins.
#[derive(Default)]
pub struct IntentRouter {
    parsers: Mutex<Vec<Arc<dyn IntentParser>>>,
}

impl IntentRouter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an [`IntentParser`]. Order matters — earlier entries
    /// win on ties (parity with `IntentService`'s ordered list).
    pub fn register<P: IntentParser + 'static>(&self, parser: P) {
        self.parsers.lock().push(Arc::new(parser));
    }

    /// Register an already-Arc'd parser (convenience for shared
    /// instances).
    pub fn register_arc(&self, parser: Arc<dyn IntentParser>) {
        self.parsers.lock().push(parser);
    }

    #[must_use]
    pub fn parser_names(&self) -> Vec<String> {
        self.parsers
            .lock()
            .iter()
            .map(|p| p.name().to_string())
            .collect()
    }

    /// Run every registered parser against the utterance and pick the
    /// highest-confidence match. Returns `None` when no parser fires.
    #[must_use]
    pub fn resolve(&self, utterance: &str, language: &str) -> Option<IntentMatch> {
        let parsers = self.parsers.lock().clone();
        let mut best: Option<IntentMatch> = None;
        for parser in parsers {
            if let Some(m) = parser.parse(utterance, language) {
                let better = best
                    .as_ref()
                    .map_or(true, |b| m.confidence > b.confidence);
                if better {
                    best = Some(m);
                }
            }
        }
        best
    }
}
