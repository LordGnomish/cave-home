// SPDX-License-Identifier: Apache-2.0
//! Skill registry.
//!
//! # Upstream:
//! `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/skill_manager.py::SkillManager` —
//! scans skill directories and instantiates each `OVOSSkill` subclass.
//! Phase 1 skills are first-party Rust types registered manually; the
//! manager surface (`load_skill` / `unload_skill` / `dispatch`) matches
//! the upstream API.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::base::{Skill, SkillContext, SkillResponse};
use crate::error::VoiceResult;
use crate::intent::IntentMatch;

/// In-memory skill registry.
///
/// # Upstream:
/// `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/skill_manager.py::SkillManager`
#[derive(Default)]
pub struct SkillLoader {
    /// Skill id → instance.
    skills: Mutex<HashMap<String, Arc<dyn Skill>>>,
    /// Intent name → skill id.
    index: Mutex<HashMap<&'static str, String>>,
}

impl SkillLoader {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a skill instance.
    ///
    /// # Upstream:
    /// `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/skill_manager.py::SkillManager.load_skill`
    pub fn load(&self, skill: Arc<dyn Skill>) {
        let id = skill.id().to_string();
        for name in skill.handled_intents() {
            self.index.lock().insert(*name, id.clone());
        }
        self.skills.lock().insert(id, skill);
    }

    /// Remove a skill (parity with `unload_skill`).
    pub fn unload(&self, id: &str) {
        self.skills.lock().remove(id);
        self.index.lock().retain(|_, v| v != id);
    }

    /// Loaded skill ids.
    #[must_use]
    pub fn list(&self) -> Vec<String> {
        self.skills.lock().keys().cloned().collect()
    }

    /// Number of loaded skills.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.lock().len()
    }

    /// True iff no skills are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.lock().is_empty()
    }

    /// Look up a skill by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<dyn Skill>> {
        self.skills.lock().get(id).cloned()
    }

    /// Find the skill that handles `intent_name`.
    #[must_use]
    pub fn for_intent(&self, intent_name: &str) -> Option<Arc<dyn Skill>> {
        let skill_id = self.index.lock().get(intent_name).cloned()?;
        self.skills.lock().get(&skill_id).cloned()
    }

    /// Dispatch an intent match to the registered skill.
    ///
    /// # Errors
    /// Propagates whatever [`Skill::handle`] returns.
    ///
    /// # Upstream:
    /// `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/intent_services/__init__.py::IntentService.handle_utterance`
    /// — final step, calling the skill's intent handler.
    pub async fn dispatch(
        &self,
        intent: &IntentMatch,
        ctx: &SkillContext,
    ) -> VoiceResult<Option<SkillResponse>> {
        let Some(skill) = self.for_intent(&intent.name) else {
            return Ok(None);
        };
        let skill_ctx = SkillContext {
            skill_id: skill.id().to_string(),
            ..ctx.clone()
        };
        skill.handle(intent, &skill_ctx).await
    }
}
