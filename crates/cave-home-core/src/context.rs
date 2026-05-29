//! Port of `homeassistant.core.Context`.
//!
//! A `Context` ties a chain of events together — when an automation fires
//! a service call, the resulting state change shares the originator's
//! context id so the trace can be reconstructed. Fields mirror upstream:
//! `id`, `user_id`, `parent_id`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub id: String,
    pub user_id: Option<String>,
    pub parent_id: Option<String>,
}

impl Context {
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            user_id: None,
            parent_id: None,
        }
    }

    #[must_use]
    pub fn with_user(user_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            user_id: Some(user_id.into()),
            parent_id: None,
        }
    }

    #[must_use]
    pub fn child_of(parent: &Self) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            user_id: parent.user_id.clone(),
            parent_id: Some(parent.id.clone()),
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_assigns_unique_ids() {
        let a = Context::new();
        let b = Context::new();
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn child_inherits_user_and_parent_id() {
        let root = Context::with_user("alice");
        let child = Context::child_of(&root);
        assert_eq!(child.user_id.as_deref(), Some("alice"));
        assert_eq!(child.parent_id.as_ref(), Some(&root.id));
        assert_ne!(child.id, root.id);
    }

    #[test]
    fn default_has_no_user_or_parent() {
        let c = Context::default();
        assert!(c.user_id.is_none());
        assert!(c.parent_id.is_none());
        assert!(!c.id.is_empty());
    }

    #[test]
    fn grandchild_chains_to_root_user_but_unique_ids() {
        // Two-deep causality chain: an automation (root) fires a service
        // call (child) that triggers another (grandchild).
        let root = Context::with_user("alice");
        let child = Context::child_of(&root);
        let grandchild = Context::child_of(&child);
        assert_eq!(grandchild.user_id.as_deref(), Some("alice"));
        assert_eq!(grandchild.parent_id.as_ref(), Some(&child.id));
        // every link in the chain has a distinct id
        assert_ne!(grandchild.id, child.id);
        assert_ne!(grandchild.id, root.id);
    }

    #[test]
    fn serde_round_trip_preserves_chain() {
        let root = Context::with_user("bob");
        let child = Context::child_of(&root);
        let s = serde_json::to_string(&child).expect("serialise");
        let back: Context = serde_json::from_str(&s).expect("deserialise");
        assert_eq!(back, child);
    }
}
