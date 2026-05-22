// SPDX-License-Identifier: Apache-2.0
//! Scene — port of `homeassistant/components/scene/__init__.py`.
//!
//! A scene is a named snapshot of entity states; activating it
//! re-applies that snapshot via the state machine.
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/components/scene/__init__.py

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::Context;
use crate::error::HassResult;
use crate::state::StateMachine;

/// A single entity's target value within a scene.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneEntry {
    pub entity_id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
}

/// Named snapshot of entity states. The grandma-friendly Portal calls
/// these **"Sahne"** (Scene).
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/components/scene/__init__.py::Scene
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    pub entries: Vec<SceneEntry>,
}

impl Scene {
    /// New empty scene.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            entries: Vec::new(),
        }
    }

    /// Add an entity-state entry.
    pub fn add(
        &mut self,
        entity_id: impl Into<String>,
        state: impl Into<String>,
        attributes: BTreeMap<String, Value>,
    ) {
        self.entries.push(SceneEntry {
            entity_id: entity_id.into(),
            state: state.into(),
            attributes,
        });
    }

    /// Activate the scene — push every entry through the state machine.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/components/scene/__init__.py::Scene.async_activate
    pub fn activate(
        &self,
        sm: &Arc<StateMachine>,
        context: Option<Context>,
    ) -> HassResult<()> {
        let ctx = context.unwrap_or_default();
        for entry in &self.entries {
            sm.async_set(
                &entry.entity_id,
                entry.state.clone(),
                entry.attributes.clone(),
                false,
                Some(ctx.clone()),
            )?;
        }
        Ok(())
    }
}

/// In-memory registry of scenes keyed by id. Backed by a `RwLock`,
/// safe to share across threads.
#[derive(Debug, Default)]
pub struct SceneRegistry {
    scenes: parking_lot::RwLock<std::collections::HashMap<String, Scene>>,
}

impl SceneRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, scene: Scene) {
        self.scenes.write().insert(scene.id.clone(), scene);
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Scene> {
        self.scenes.read().get(id).cloned()
    }

    pub fn remove(&self, id: &str) -> Option<Scene> {
        self.scenes.write().remove(id)
    }

    pub fn list(&self) -> Vec<Scene> {
        self.scenes.read().values().cloned().collect()
    }

    pub fn activate(
        &self,
        id: &str,
        sm: &Arc<StateMachine>,
        context: Option<Context>,
    ) -> HassResult<()> {
        let scene = self
            .get(id)
            .ok_or_else(|| crate::error::HassError::Other(format!("unknown scene: {id}")))?;
        scene.activate(sm, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::InMemoryEventBus;

    fn machine() -> Arc<StateMachine> {
        Arc::new(StateMachine::new(Arc::new(InMemoryEventBus::new())))
    }

    /// Upstream-test: `tests/components/scene/test_init.py::test_apply_service`
    #[test]
    fn scene_activate_sets_states() {
        let sm = machine();
        let mut scene = Scene::new("evening", "Akşam");
        scene.add("light.kitchen", "on", BTreeMap::new());
        scene.add("light.living_room", "on", {
            let mut a = BTreeMap::new();
            a.insert("brightness".into(), Value::from(120));
            a
        });
        scene.activate(&sm, None).unwrap();
        assert!(sm.is_state("light.kitchen", "on"));
        let lr = sm.get("light.living_room").unwrap();
        assert_eq!(lr.state, "on");
        assert_eq!(lr.attributes.get("brightness"), Some(&Value::from(120)));
    }
}
