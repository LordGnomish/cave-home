//! Port of `homeassistant.components.homeassistant.scene`.
//!
//! A scene is a named snapshot of target entity states. Activating it writes
//! each target into the [`StateMachine`](crate::state_machine), sharing one
//! [`Context`] across the writes so the whole activation is one traceable
//! causal group — HA's `async_activate`.

use crate::context::Context;
use crate::state::{EntityId, StateAttributes};
use crate::state_machine::{StateChange, StateMachine};
use crate::util::{ensure_unique_string, slugify};
use parking_lot::RwLock;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SceneError {
    #[error("scene name must not be empty")]
    EmptyName,
    #[error("no scene with id {0:?}")]
    UnknownId(String),
}

/// A single entity target within a scene.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneEntityState {
    pub state: String,
    pub attributes: StateAttributes,
}

impl SceneEntityState {
    #[must_use]
    pub fn new(state: impl Into<String>) -> Self {
        Self { state: state.into(), attributes: StateAttributes::new() }
    }

    #[must_use]
    pub fn with_attr(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }
}

/// Port of a `scene` config entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scene {
    pub id: String,
    pub name: String,
    /// Target state for each entity, applied on activation.
    pub entity_states: BTreeMap<EntityId, SceneEntityState>,
}

impl Scene {
    /// Activate the scene: write every target into `states` under one shared
    /// child [`Context`] of `parent`. Returns the [`StateChange`]s produced
    /// (entities already in their target state yield no change, as upstream).
    #[must_use]
    pub fn apply(&self, states: &StateMachine, parent: &Context) -> Vec<StateChange> {
        // One shared context for the whole activation so every resulting
        // state_changed traces back to the same scene-activation cause.
        let context = Context::child_of(parent);
        self.entity_states
            .iter()
            .filter_map(|(entity_id, target)| {
                states.set(
                    entity_id.clone(),
                    target.state.clone(),
                    target.attributes.clone(),
                    context.clone(),
                )
            })
            .collect()
    }
}

#[derive(Default)]
struct SceneInner {
    scenes: BTreeMap<String, Scene>,
}

/// Registry of [`Scene`]s.
#[derive(Clone, Default)]
pub struct SceneRegistry {
    inner: Arc<RwLock<SceneInner>>,
}

impl SceneRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a scene from a name (slug id) and its target states.
    ///
    /// # Errors
    /// [`SceneError::EmptyName`] if `name` slugs to nothing.
    pub fn create(
        &self,
        name: impl Into<String>,
        entity_states: BTreeMap<EntityId, SceneEntityState>,
    ) -> Result<Scene, SceneError> {
        let name = name.into();
        let slug = slugify(&name);
        if slug.is_empty() {
            return Err(SceneError::EmptyName);
        }
        let mut guard = self.inner.write();
        let existing: HashSet<String> = guard.scenes.keys().cloned().collect();
        let id = ensure_unique_string(&slug, &existing);
        let scene = Scene { id: id.clone(), name, entity_states };
        guard.scenes.insert(id, scene.clone());
        Ok(scene)
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Scene> {
        self.inner.read().scenes.get(id).cloned()
    }

    #[must_use]
    pub fn list(&self) -> Vec<Scene> {
        self.inner.read().scenes.values().cloned().collect()
    }

    /// Activate scene `id` against `states`.
    ///
    /// # Errors
    /// [`SceneError::UnknownId`] if no scene with that id exists.
    pub fn activate(
        &self,
        id: &str,
        states: &StateMachine,
        parent: &Context,
    ) -> Result<Vec<StateChange>, SceneError> {
        let scene = self.get(id).ok_or_else(|| SceneError::UnknownId(id.to_owned()))?;
        Ok(scene.apply(states, parent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn light(object: &str) -> EntityId {
        EntityId::new("light", object).expect("id")
    }

    fn movie_targets() -> BTreeMap<EntityId, SceneEntityState> {
        let mut m = BTreeMap::new();
        m.insert(light("kitchen"), SceneEntityState::new("off"));
        m.insert(
            light("lounge"),
            SceneEntityState::new("on").with_attr("brightness", json!(40)),
        );
        m
    }

    #[test]
    fn apply_writes_targets_and_shares_context() {
        let states = StateMachine::new(crate::event_bus::EventBus::new());
        // seed initial states
        states.set(light("kitchen"), "on", StateAttributes::new(), Context::new());
        states.set(light("lounge"), "off", StateAttributes::new(), Context::new());

        let scene = Scene {
            id: "movie".into(),
            name: "Movie".into(),
            entity_states: movie_targets(),
        };
        let parent = Context::with_user("alice");
        let changes = scene.apply(&states, &parent);

        // both entities changed
        assert_eq!(changes.len(), 2);
        assert!(states.is_state(&light("kitchen"), "off"));
        assert!(states.is_state(&light("lounge"), "on"));
        assert_eq!(
            states.get(&light("lounge")).map(|s| s.attributes["brightness"].clone()),
            Some(json!(40))
        );
        // every write shares one context that descends from the parent
        for ch in &changes {
            assert_eq!(ch.new_state.context.parent_id.as_ref(), Some(&parent.id));
        }
        let ctx_ids: HashSet<String> = changes.iter().map(|c| c.new_state.context.id.clone()).collect();
        assert_eq!(ctx_ids.len(), 1, "all writes share one context id");
    }

    #[test]
    fn apply_skips_entities_already_in_target_state() {
        let states = StateMachine::new(crate::event_bus::EventBus::new());
        // kitchen is already off — applying its "off" target is a no-op
        states.set(light("kitchen"), "off", StateAttributes::new(), Context::new());
        states.set(light("lounge"), "off", StateAttributes::new(), Context::new());

        let scene = Scene { id: "movie".into(), name: "Movie".into(), entity_states: movie_targets() };
        let changes = scene.apply(&states, &Context::new());
        // only lounge changed (kitchen was already off)
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity_id, light("lounge"));
    }

    #[test]
    fn registry_create_and_activate() {
        let reg = SceneRegistry::new();
        let scene = reg.create("Movie Night", movie_targets()).expect("create");
        assert_eq!(scene.id, "movie_night");

        let states = StateMachine::new(crate::event_bus::EventBus::new());
        let changes = reg.activate(&scene.id, &states, &Context::new()).expect("activate");
        // both targets were fresh writes
        assert_eq!(changes.len(), 2);
        assert!(states.is_state(&light("kitchen"), "off"));

        assert_eq!(
            reg.activate("ghost", &states, &Context::new()).unwrap_err(),
            SceneError::UnknownId("ghost".into())
        );
    }

    #[test]
    fn empty_name_rejected() {
        let reg = SceneRegistry::new();
        assert_eq!(reg.create("  ", BTreeMap::new()).unwrap_err(), SceneError::EmptyName);
    }
}
