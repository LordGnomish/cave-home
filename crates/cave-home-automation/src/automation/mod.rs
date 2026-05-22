// SPDX-License-Identifier: Apache-2.0
//! Automation engine — port of
//! `homeassistant/components/automation/__init__.py` plus the
//! associated triggers / conditions / actions modules.
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/components/automation/__init__.py

pub mod conditions;
pub mod engine;
pub mod triggers;

pub use conditions::Condition;
pub use engine::{Automation, AutomationConfig, AutomationEngine, AutomationHandle};
pub use triggers::Trigger;

// Re-export script Action so the prelude is complete.
pub use crate::script::Action;
