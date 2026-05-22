// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Scenes cluster (0x0005) — ZCL §3.7.
//!
//! A *Scene* records the attribute values cluster-by-cluster for the
//! members of a group; recalling the scene restores those values. The
//! headline persona experiences scenes as the "Akşam" / "Sabah" tile in
//! the Portal — pressing it dims the lights and sets the colour.

use std::collections::BTreeMap;

use crate::error::{Result, ZigbeeError};

/// Scenes cluster identifier — ZCL §3.7.1.
pub const SCENES_CLUSTER_ID: u16 = 0x0005;

/// Command identifiers — ZCL §3.7.2.1.
pub mod command_id {
    /// Add Scene.
    pub const ADD: u8 = 0x00;
    /// View Scene.
    pub const VIEW: u8 = 0x01;
    /// Remove Scene.
    pub const REMOVE: u8 = 0x02;
    /// Remove All Scenes.
    pub const REMOVE_ALL: u8 = 0x03;
    /// Store Scene.
    pub const STORE: u8 = 0x04;
    /// Recall Scene.
    pub const RECALL: u8 = 0x05;
    /// Get Scene Membership.
    pub const GET_MEMBERSHIP: u8 = 0x06;
}

/// One scene — a (group, scene id, name, transition time, per-cluster attribute payload).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scene {
    /// Group ID this scene belongs to (0 = "no group", per ZCL §3.7.1).
    pub group_id: u16,
    /// Scene ID within the group (1..=255 per ZCL §3.7.1).
    pub scene_id: u8,
    /// Human-readable name — surfaced as "Sahne" in the Portal.
    pub name: String,
    /// Transition time, 1/10ths of a second.
    pub transition_time_tenths: u16,
    /// Extension field — per-cluster attribute snapshots (raw, per ZCL §3.7.2.5).
    pub extension_field_sets: Vec<u8>,
}

impl Scene {
    /// Construct a scene with default (empty) extension fields.
    #[must_use]
    pub fn new(group_id: u16, scene_id: u8, name: impl Into<String>) -> Self {
        Self {
            group_id,
            scene_id,
            name: name.into(),
            transition_time_tenths: 0,
            extension_field_sets: Vec::new(),
        }
    }
}

/// Scenes table — Phase 1 backing store for the Scenes cluster.
#[derive(Clone, Debug, Default)]
pub struct ScenesCluster {
    scenes: BTreeMap<(u16, u8), Scene>,
}

impl ScenesCluster {
    /// Empty scenes table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) a scene.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Network`] if `scene.scene_id == 0`.
    pub fn add(&mut self, scene: Scene) -> Result<()> {
        if scene.scene_id == 0 {
            return Err(ZigbeeError::Network("scene_id 0 reserved".into()));
        }
        self.scenes.insert((scene.group_id, scene.scene_id), scene);
        Ok(())
    }

    /// View a scene by (group, scene) id.
    #[must_use]
    pub fn view(&self, group_id: u16, scene_id: u8) -> Option<&Scene> {
        self.scenes.get(&(group_id, scene_id))
    }

    /// Remove a scene. Returns `true` if it existed.
    pub fn remove(&mut self, group_id: u16, scene_id: u8) -> bool {
        self.scenes.remove(&(group_id, scene_id)).is_some()
    }

    /// Remove every scene that belongs to `group_id`. Returns the number dropped.
    pub fn remove_all_in_group(&mut self, group_id: u16) -> usize {
        let keys: Vec<(u16, u8)> = self
            .scenes
            .keys()
            .filter(|(g, _)| *g == group_id)
            .copied()
            .collect();
        let n = keys.len();
        for k in keys {
            self.scenes.remove(&k);
        }
        n
    }

    /// All scene IDs in `group_id` (sorted ascending).
    #[must_use]
    pub fn membership(&self, group_id: u16) -> Vec<u8> {
        self.scenes
            .keys()
            .filter(|(g, _)| *g == group_id)
            .map(|(_, s)| *s)
            .collect()
    }

    /// Snapshot of every scene.
    #[must_use]
    pub fn list(&self) -> Vec<Scene> {
        self.scenes.values().cloned().collect()
    }

    /// Recall a scene — returns a borrow of the scene to recall, or
    /// `None` if it doesn't exist. The caller is responsible for
    /// issuing the actual ZCL commands to restore the attribute values
    /// (because that needs the live coordinator handle).
    #[must_use]
    pub fn recall(&self, group_id: u16, scene_id: u8) -> Option<&Scene> {
        self.view(group_id, scene_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_view() {
        let mut c = ScenesCluster::new();
        c.add(Scene::new(1, 1, "Akşam")).unwrap();
        assert_eq!(c.view(1, 1).unwrap().name, "Akşam");
    }

    #[test]
    fn add_rejects_scene_id_zero() {
        let mut c = ScenesCluster::new();
        assert!(c.add(Scene::new(1, 0, "x")).is_err());
    }

    #[test]
    fn remove_returns_true_only_when_present() {
        let mut c = ScenesCluster::new();
        c.add(Scene::new(1, 2, "x")).unwrap();
        assert!(c.remove(1, 2));
        assert!(!c.remove(1, 2));
    }

    #[test]
    fn remove_all_in_group_targets_only_that_group() {
        let mut c = ScenesCluster::new();
        c.add(Scene::new(1, 1, "a")).unwrap();
        c.add(Scene::new(1, 2, "b")).unwrap();
        c.add(Scene::new(2, 1, "c")).unwrap();
        assert_eq!(c.remove_all_in_group(1), 2);
        assert!(c.view(2, 1).is_some());
    }

    #[test]
    fn membership_is_sorted() {
        let mut c = ScenesCluster::new();
        c.add(Scene::new(1, 3, "x")).unwrap();
        c.add(Scene::new(1, 1, "x")).unwrap();
        c.add(Scene::new(1, 2, "x")).unwrap();
        assert_eq!(c.membership(1), vec![1, 2, 3]);
    }

    #[test]
    fn recall_returns_view() {
        let mut c = ScenesCluster::new();
        let s = Scene::new(1, 1, "Sabah");
        c.add(s.clone()).unwrap();
        assert_eq!(c.recall(1, 1), Some(&s));
    }

    #[test]
    fn recall_unknown_returns_none() {
        let c = ScenesCluster::new();
        assert!(c.recall(1, 99).is_none());
    }
}
