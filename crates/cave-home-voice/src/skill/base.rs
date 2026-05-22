// SPDX-License-Identifier: Apache-2.0
//! Skill base trait.
//!
//! # Upstream:
//! - `OpenVoiceOS/ovos-workshop@9ddd6f8:ovos_workshop/skills/ovos.py::OVOSSkill.__init__` —
//!   the upstream class stores `bus`, `skill_id`, `lang`, and exposes
//!   `speak()` / `get_response()`. We collapse to a trait + context
//!   pair (Rust idiom).
//! - `OpenVoiceOS/ovos-workshop@9ddd6f8:ovos_workshop/skills/ovos.py::OVOSSkill.handle_intent` —
//!   intent-dispatch entry point. Our trait surface is
//!   [`Skill::handle`].

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::bus::VoiceBus;
use crate::dialog::DialogRenderer;
use crate::error::VoiceResult;
use crate::intent::IntentMatch;

/// What a skill returns when it handles an intent.
///
/// # Upstream:
/// `OpenVoiceOS/ovos-workshop@9ddd6f8:ovos_workshop/skills/ovos.py::OVOSSkill.speak`
/// — upstream pushes a `speak` message onto the bus with the rendered
/// dialog text and metadata. We return a struct so the caller (the
/// pipeline) can push the bus message and trigger TTS in one shot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillResponse {
    pub utterance: String,
    pub language: String,
    /// Optional voice override (skill-specific persona).
    pub voice: Option<String>,
    /// Bus metadata to attach to the outgoing `speak` envelope.
    pub meta: HashMap<String, String>,
}

impl SkillResponse {
    /// Convenience constructor.
    #[must_use]
    pub fn new<S: Into<String>>(utterance: S, language: &str) -> Self {
        Self {
            utterance: utterance.into(),
            language: language.into(),
            voice: None,
            meta: HashMap::new(),
        }
    }
}

/// Per-invocation context handed to a skill.
///
/// # Upstream:
/// `OpenVoiceOS/ovos-workshop@9ddd6f8:ovos_workshop/skills/ovos.py::OVOSSkill.bus`
/// + `lang` + `skill_id` — same trio, packaged into one struct.
#[derive(Clone)]
pub struct SkillContext {
    pub bus: VoiceBus,
    pub dialog: Arc<DialogRenderer>,
    pub skill_id: String,
    pub language: String,
}

impl SkillContext {
    /// Render a dialog template against the supplied slots.
    ///
    /// # Upstream:
    /// `OpenVoiceOS/ovos-workshop@9ddd6f8:ovos_workshop/skills/ovos.py::OVOSSkill.dialog_renderer`
    #[must_use]
    pub fn render(&self, template: &str, slots: &HashMap<String, String>) -> Option<String> {
        self.dialog.render(template, slots)
    }
}

/// A loaded skill.
///
/// Implementors are `Send + Sync` so the skill manager can hold
/// `Arc<dyn Skill>`. Phase 1 skills are first-party Rust modules; Phase
/// 1b will add a sandboxed external loader (Wasm).
#[async_trait]
pub trait Skill: Send + Sync {
    /// Skill identifier (`lighting`, `clock`, …). Mirrors
    /// `OVOSSkill.skill_id`.
    fn id(&self) -> &str;

    /// Names of intents this skill claims. The skill loader uses this
    /// to short-circuit dispatch when an intent name has no handler.
    fn handled_intents(&self) -> &[&'static str];

    /// Run an intent. Returns `Ok(None)` when the skill declines the
    /// match (used by the loader to try a fallback).
    async fn handle(
        &self,
        intent: &IntentMatch,
        ctx: &SkillContext,
    ) -> VoiceResult<Option<SkillResponse>>;
}

/// Convenience macro for declaring intent handlers on a skill struct.
///
/// # Upstream:
/// `OpenVoiceOS/ovos-workshop@9ddd6f8:ovos_workshop/decorators/__init__.py::intent_handler`
///
/// Usage:
/// ```ignore
/// intent_handler!(my_skill, "lighting.turn_on", |intent, ctx| async move {
///     Ok(Some(SkillResponse::new("Tamam, ışıkları açıyorum.", &ctx.language)))
/// });
/// ```
#[macro_export]
macro_rules! intent_handler {
    ($skill:expr, $name:expr, |$intent:ident, $ctx:ident| $body:expr) => {{
        let _name: &'static str = $name;
        // Stable surface: returns an `(&'static str, BoxedHandler)` pair
        // the caller can stash in their own dispatch map. Mirrors the
        // upstream `@intent_handler('lighting.TurnOnIntent')` decorator
        // which appends a `(intent_name, callable)` tuple to the class
        // namespace.
        ($name, move |$intent: &$crate::intent::IntentMatch,
                      $ctx: &$crate::skill::SkillContext| {
            let f: ::std::pin::Pin<
                Box<dyn ::std::future::Future<Output = $crate::error::VoiceResult<Option<$crate::skill::SkillResponse>>>
                    + Send>,
            > = Box::pin($body);
            f
        })
    }};
}

pub use intent_handler;
