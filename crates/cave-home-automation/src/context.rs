// SPDX-License-Identifier: Apache-2.0
//! Causal `Context` — ported from `homeassistant/core.py`.
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::Context

use serde::{Deserialize, Serialize};

/// The context that triggered something — every state change, every
/// service call, every event carries one.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::Context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    /// ULID-style monotonic identifier; the Python implementation uses
    /// `ulid_now()` / `ulid_at_time()`.
    pub id: String,
    /// User that initiated the action, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Parent context id (e.g. the event that caused this state change).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

impl Context {
    /// New `Context` with a freshly-generated id.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::Context.__init__
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            user_id: None,
            parent_id: None,
        }
    }

    /// New `Context` with explicit user/parent links.
    #[must_use]
    pub fn with_links(user_id: Option<String>, parent_id: Option<String>) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            user_id,
            parent_id,
        }
    }

    /// New `Context` with caller-supplied id (round-trips through JSON).
    #[must_use]
    pub fn with_id(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            user_id: None,
            parent_id: None,
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Context {
    /// Equality is by `id` alone — matches HA core's `Context.__eq__`.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::Context.__eq__
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Context {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_ids_are_unique() {
        let a = Context::new();
        let b = Context::new();
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn context_equality_is_by_id() {
        let a = Context::with_id("abc");
        let b = Context::with_id("abc");
        let c = Context::with_id("def");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn context_serializes_with_id() {
        let ctx = Context::with_id("01H").with_links_helper("user-1", "parent-1");
        let json = serde_json::to_value(&ctx).unwrap();
        assert_eq!(json["id"], "01H");
        assert_eq!(json["user_id"], "user-1");
        assert_eq!(json["parent_id"], "parent-1");
    }

    impl Context {
        fn with_links_helper(mut self, user: &str, parent: &str) -> Self {
            self.user_id = Some(user.into());
            self.parent_id = Some(parent.into());
            self
        }
    }
}
