// SPDX-License-Identifier: Apache-2.0
//! OVOS skill framework — base trait + loader + handler registry.
//!
//! # Upstream:
//! - `OpenVoiceOS/ovos-workshop@9ddd6f8:ovos_workshop/skills/ovos.py::OVOSSkill` —
//!   base class; ported to [`base::Skill`] trait.
//! - `OpenVoiceOS/ovos-workshop@9ddd6f8:ovos_workshop/decorators/__init__.py::intent_handler` —
//!   decorator that attaches an `Intent` to a method. We surface this
//!   via the [`base::SkillHandler`] registration API; the macro-rules
//!   syntax in [`base::intent_handler!`] reproduces the upstream
//!   ergonomics without needing a proc-macro crate.
//! - `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/skill_manager.py::SkillManager.load_skill` —
//!   loader; ported to [`loader::SkillLoader`].

pub mod base;
pub mod loader;

pub use base::{Skill, SkillContext, SkillResponse};
pub use loader::SkillLoader;
