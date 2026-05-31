// SPDX-License-Identifier: Apache-2.0
//! `cave-home-voice` — the local-first voice **intent engine** (ADR-024).
//!
//! This crate is the natural-language brain of cave-home's voice pillar: it
//! turns a recognised spoken sentence ("turn the kitchen light on") into a
//! typed command the rest of the house executes, and generates the assistant's
//! grandma-friendly spoken reply — all on-device, in English, German or Turkish
//! (Charter §6.3). It is the same sentence-template approach Rhasspy and Home
//! Assistant's "Assist" use, implemented clean-room and std-only (no regex
//! crate, no network, no cloud — Charter §9).
//!
//! # Scope (Phase-1 MVP)
//!
//! Implemented, real and tested here:
//! - [`template`] — the sentence-template grammar (`[optional]`,
//!   `(alternatives)`, `{slots}`) parser/compiler.
//! - [`slot`] — slot definitions: fixed value lists with synonyms, bounded
//!   numbers, open capture; validation + canonicalisation.
//! - [`number_words`] — spoken-number parsing ("fifty" → 50) for EN/DE/TR.
//! - [`matcher`] — match an utterance to the best intent, extract slots,
//!   report no-match / ambiguity.
//! - [`intents`] — the built-in command set (lights, climate, covers, scenes,
//!   state queries) with EN/DE/TR sentence sets.
//! - [`route`] — map a matched intent to a typed [`route::IntentAction`].
//! - [`response`] — generate the localised spoken reply.
//! - [`config`] — the wake-word + assistant configuration model (settings
//!   only, validated).
//! - [`label`] — the supported-language tag.
//!
//! # Deferred to Phase-1b (ML / audio / network-bound — see `parity.manifest.toml`)
//!
//! The speech-to-text engine (whisper.cpp-class), text-to-speech engine
//! (piper-class), wake-word detection (openWakeWord-class), the audio capture
//! pipeline + voice-activity detection, per-user voice profiles, and the
//! cave-home-core execution wiring are all model/hardware-bound and are
//! enumerated as `[[unmapped]]` with an ADR-024 disposition. cave-home never
//! ships a cloud STT/TTS path (Charter §9) — that exclusion is `permanent`.
//!
//! # Example
//!
//! ```
//! use cave_home_voice::{understand, Lang, Understanding};
//! use cave_home_voice::route::IntentAction;
//!
//! let intents = cave_home_voice::intents::builtin_intents().expect("built-ins");
//!
//! match understand("turn the kitchen light on", &intents, Lang::En) {
//!     Understanding::Acted { action, reply, .. } => {
//!         assert_eq!(
//!             action,
//!             IntentAction::SetLight { target: "kitchen".into(), on: true }
//!         );
//!         assert_eq!(reply, "Turning on the kitchen light.");
//!     }
//!     other => panic!("expected an action, got {other:?}"),
//! }
//! ```

pub mod config;
pub mod intents;
pub mod label;
pub mod matcher;
pub mod number_words;
pub mod policy;
pub mod response;
pub mod route;
pub mod slot;
pub mod template;

pub use config::{AssistantConfig, ConfigError, WakeWord};
pub use label::Lang;
pub use matcher::{match_intent, CompiledIntent, IntentMatch, MatchOutcome};
pub use policy::{authorize, sensitivity, Decision, PermissionLevel, Sensitivity};
pub use route::{IntentAction, RoutedAction};
pub use slot::{SlotKind, SlotValue, ValueList};
pub use template::Template;

use response::Answer;

/// The end-to-end outcome of understanding one utterance: matched + routed +
/// a spoken reply, or a graceful fallback. This is what the voice front-end
/// (Phase-1b) consumes per recognised sentence.
#[derive(Debug, Clone, PartialEq)]
pub enum Understanding {
    /// A command was understood and routed to an action.
    Acted {
        /// The typed action for the rest of cave-home to execute.
        action: IntentAction,
        /// The grandma-friendly spoken reply.
        reply: String,
        /// Match confidence (specificity), carried through from the matcher.
        confidence: f32,
    },
    /// Nothing matched — the assistant asks the household to rephrase.
    NotUnderstood {
        /// The spoken reply.
        reply: String,
    },
    /// Several intents tied — the assistant asks for clarification.
    NeedsClarification {
        /// The tied intent ids (not user-facing).
        candidates: Vec<String>,
        /// The spoken reply.
        reply: String,
    },
}

/// Understand one recognised utterance against a compiled intent set, producing
/// a typed action + a spoken reply (or a graceful fallback).
///
/// `lang` selects the language of the spoken reply. Query actions are answered
/// with "I can't tell right now" here, because reading live state is the
/// caller's job (Phase-1b core wiring); see [`response::respond`] to supply an
/// [`response::Answer`] once the state is known.
#[must_use]
pub fn understand(utterance: &str, intents: &[CompiledIntent], lang: Lang) -> Understanding {
    match match_intent(utterance, intents) {
        MatchOutcome::Matched(m) => match route::route(&m) {
            Ok(routed) => {
                let reply = response::respond(&routed.action, lang, Answer::default());
                Understanding::Acted {
                    action: routed.action,
                    reply,
                    confidence: routed.confidence,
                }
            }
            // Matched an intent the router doesn't action — treat as not
            // understood rather than surfacing a routing error to the user.
            Err(_) => Understanding::NotUnderstood {
                reply: response::not_understood(lang),
            },
        },
        MatchOutcome::Ambiguous(candidates) => Understanding::NeedsClarification {
            candidates,
            reply: response::please_clarify(lang),
        },
        MatchOutcome::NoMatch => Understanding::NotUnderstood {
            reply: response::not_understood(lang),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn understand_acts_on_a_command() {
        let intents = intents::builtin_intents().expect("c");
        match understand("turn the kitchen light on", &intents, Lang::En) {
            Understanding::Acted { action, reply, confidence } => {
                assert_eq!(
                    action,
                    IntentAction::SetLight {
                        target: "kitchen".into(),
                        on: true
                    }
                );
                assert_eq!(reply, "Turning on the kitchen light.");
                assert!(confidence > 0.5);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn understand_falls_back_on_gibberish() {
        let intents = intents::builtin_intents().expect("c");
        match understand("wibble wobble", &intents, Lang::De) {
            Understanding::NotUnderstood { reply } => assert!(!reply.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn understand_replies_in_requested_language() {
        let intents = intents::builtin_intents().expect("c");
        match understand("mutfak ışığını aç", &intents, Lang::Tr) {
            Understanding::Acted { reply, .. } => assert!(reply.contains("açıyorum")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn understand_reports_ambiguity() {
        // Two distinct intents that both match the same bare phrase.
        let a = CompiledIntent::new(
            "alpha",
            Lang::En,
            "{x}",
            std::iter::once(("x".to_string(), SlotKind::Open)).collect(),
        )
        .expect("t");
        let b = CompiledIntent::new(
            "beta",
            Lang::En,
            "{y}",
            std::iter::once(("y".to_string(), SlotKind::Open)).collect(),
        )
        .expect("t");
        match understand("hello", &[a, b], Lang::En) {
            Understanding::NeedsClarification { candidates, reply } => {
                assert_eq!(candidates.len(), 2);
                assert!(!reply.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }
}
