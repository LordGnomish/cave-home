// SPDX-License-Identifier: Apache-2.0
//! Common intent parser trait.
//!
//! # Upstream:
//! `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/intent_services/base.py::IntentService`
//! — every concrete intent backend implements a `match(utterance, lang)`
//! method that returns either `None` or a typed match record. We use
//! the same shape here.

use super::IntentMatch;

/// Strategy interface for an intent backend.
///
/// Implementations are `Send + Sync` so the router can hold
/// `Arc<dyn IntentParser>`.
pub trait IntentParser: Send + Sync {
    /// Backend name (`adapt`, `padatious`, …).
    fn name(&self) -> &'static str;

    /// Attempt to match. Returns the highest-confidence match this
    /// parser found, or `None` when the parser does not fire.
    fn parse(&self, utterance: &str, language: &str) -> Option<IntentMatch>;
}
