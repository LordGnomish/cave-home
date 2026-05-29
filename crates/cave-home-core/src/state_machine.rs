//! Port of `homeassistant.core.StateMachine`.
//!
//! Authoritative store of every entity's current `State`. On every
//! `set`, the machine derives `last_changed` from the previous state
//! (carrying it forward when only attributes changed) and fires a
//! `state_changed` event on the bus.

use crate::context::Context;
use crate::event::{Event, EventOrigin};
use crate::event_bus::EventBus;
use crate::state::{EntityId, EntityIdError, State, StateAttributes};
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use time::OffsetDateTime;

pub const EVENT_STATE_CHANGED: &str = "state_changed";

#[derive(Debug, Error)]
pub enum StateMachineError {
    #[error("entity id error: {0}")]
    EntityId(#[from] EntityIdError),
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateChange {
    pub entity_id: EntityId,
    pub old_state: Option<State>,
    pub new_state: State,
}

#[derive(Clone, Default)]
pub struct StateMachine {
    inner: Arc<RwLock<HashMap<EntityId, State>>>,
    bus: EventBus,
}

impl StateMachine {
    #[must_use]
    pub fn new(bus: EventBus) -> Self {
        Self { inner: Arc::new(RwLock::new(HashMap::new())), bus }
    }

    pub fn get(&self, id: &EntityId) -> Option<State> {
        self.inner.read().get(id).cloned()
    }

    pub fn is_state(&self, id: &EntityId, expected: &str) -> bool {
        self.get(id).is_some_and(|s| s.state == expected)
    }

    /// Set state. Returns the `StateChange` produced — `None` if the new
    /// state is byte-identical to the old (HA short-circuits this path).
    pub fn set(
        &self,
        id: EntityId,
        new_state: impl Into<String>,
        attributes: StateAttributes,
        context: Context,
    ) -> Option<StateChange> {
        let new_state: String = new_state.into();
        let now = OffsetDateTime::now_utc();

        let mut guard = self.inner.write();
        let old = guard.get(&id).cloned();

        if let Some(ref prev) = old {
            if prev.state == new_state && prev.attributes == attributes {
                return None;
            }
        }

        let last_changed = match old.as_ref() {
            Some(prev) if prev.state == new_state => prev.last_changed,
            _ => now,
        };

        let state = State {
            entity_id: id.clone(),
            state: new_state,
            attributes,
            last_changed,
            last_updated: now,
            context: context.clone(),
        };
        guard.insert(id.clone(), state.clone());
        drop(guard);

        let payload = json!({
            "entity_id": id.to_string(),
            "old_state": old,
            "new_state": state,
        });
        self.bus.fire(Event::new(EVENT_STATE_CHANGED, payload, EventOrigin::Local, context));

        Some(StateChange { entity_id: id, old_state: old, new_state: state })
    }

    pub fn remove(&self, id: &EntityId) -> Option<State> {
        self.inner.write().remove(id)
    }

    /// Every entity id currently tracked, in arbitrary order.
    ///
    /// Mirrors HA's `StateMachine.async_entity_ids()`.
    #[must_use]
    pub fn entity_ids(&self) -> Vec<EntityId> {
        self.inner.read().keys().cloned().collect()
    }

    /// Every tracked entity id whose domain matches `domain`
    /// (e.g. `"light"`), in arbitrary order.
    ///
    /// Mirrors HA's `StateMachine.async_entity_ids(domain_filter)`.
    #[must_use]
    pub fn entity_ids_by_domain(&self, domain: &str) -> Vec<EntityId> {
        self.inner
            .read()
            .keys()
            .filter(|id| id.domain == domain)
            .cloned()
            .collect()
    }

    /// A snapshot of every current `State`, in arbitrary order.
    ///
    /// Mirrors HA's `StateMachine.async_all()`.
    #[must_use]
    pub fn all(&self) -> Vec<State> {
        self.inner.read().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light_kitchen() -> EntityId {
        EntityId::new("light", "kitchen").expect("entity id")
    }

    #[tokio::test]
    async fn set_emits_and_short_circuits() {
        let bus = EventBus::new();
        let (_id, mut rx) = bus.listen(EVENT_STATE_CHANGED);
        let m = StateMachine::new(bus);

        let first = m.set(light_kitchen(), "on", StateAttributes::new(), Context::new()).expect("emit");
        assert!(first.old_state.is_none());
        assert_eq!(rx.recv().await.expect("evt").data["entity_id"], "light.kitchen");

        // byte-identical set must not emit
        assert!(m.set(light_kitchen(), "on", StateAttributes::new(), Context::new()).is_none());
    }

    #[tokio::test]
    async fn attribute_only_change_keeps_last_changed_but_updates_last_updated() {
        let m = StateMachine::new(EventBus::new());
        let first = m.set(light_kitchen(), "on", StateAttributes::new(), Context::new()).expect("emit");
        let lc = first.new_state.last_changed;
        let mut attrs = StateAttributes::new();
        attrs.insert("brightness".into(), json!(128));
        let second = m.set(light_kitchen(), "on", attrs, Context::new()).expect("attr emit");
        // last_changed carries forward (value did not change) ...
        assert_eq!(second.new_state.last_changed, lc);
        // ... but last_updated is at least the carried last_changed (it was
        // stamped at the report time, which is >= the original change time).
        assert!(second.new_state.last_updated >= lc);
        // attribute is now reflected
        assert_eq!(second.new_state.attributes["brightness"], 128);
    }

    #[tokio::test]
    async fn value_change_resets_last_changed_and_carries_context() {
        let m = StateMachine::new(EventBus::new());
        let on = m.set(light_kitchen(), "on", StateAttributes::new(), Context::new()).expect("on");
        let lc_on = on.new_state.last_changed;
        let ctx = Context::with_user("alice");
        let off = m.set(light_kitchen(), "off", StateAttributes::new(), ctx.clone()).expect("off");
        // a real value change advances last_changed beyond the previous one
        assert!(off.new_state.last_changed >= lc_on);
        assert_eq!(off.old_state.as_ref().map(|s| s.state.as_str()), Some("on"));
        // the supplied context rides into the new State
        assert_eq!(off.new_state.context.user_id.as_deref(), Some("alice"));
    }

    #[test]
    fn get_is_state_and_remove() {
        let m = StateMachine::new(EventBus::new());
        let id = light_kitchen();
        assert!(m.get(&id).is_none());
        assert!(!m.is_state(&id, "on"));

        m.set(id.clone(), "on", StateAttributes::new(), Context::new()).expect("set");
        assert!(m.is_state(&id, "on"));
        assert!(!m.is_state(&id, "off"));
        assert_eq!(m.get(&id).map(|s| s.state), Some("on".to_owned()));

        let removed = m.remove(&id).expect("removed state");
        assert_eq!(removed.state, "on");
        assert!(m.get(&id).is_none());
        assert!(m.remove(&id).is_none());
    }

    #[test]
    fn entity_ids_all_and_domain_query() {
        let m = StateMachine::new(EventBus::new());
        let ctx = Context::new();
        m.set(EntityId::new("light", "kitchen").expect("id"), "on", StateAttributes::new(), ctx.clone());
        m.set(EntityId::new("light", "hall").expect("id"), "off", StateAttributes::new(), ctx.clone());
        m.set(EntityId::new("lock", "front").expect("id"), "locked", StateAttributes::new(), ctx);

        // entity_ids() returns every tracked id (order-independent).
        let mut all_ids = m.entity_ids();
        all_ids.sort();
        assert_eq!(
            all_ids,
            vec![
                EntityId::new("light", "hall").expect("id"),
                EntityId::new("light", "kitchen").expect("id"),
                EntityId::new("lock", "front").expect("id"),
            ]
        );

        // entity_ids_by_domain() filters to one domain.
        let mut lights = m.entity_ids_by_domain("light");
        lights.sort();
        assert_eq!(
            lights,
            vec![
                EntityId::new("light", "hall").expect("id"),
                EntityId::new("light", "kitchen").expect("id"),
            ]
        );
        assert!(m.entity_ids_by_domain("does_not_exist").is_empty());

        // all() snapshots every current State.
        let mut all = m.all();
        all.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].entity_id.to_string(), "light.hall");
        assert_eq!(all[2].state, "locked");
    }
}
