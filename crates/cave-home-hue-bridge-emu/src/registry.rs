// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! In-memory registry of emulated lights / groups / scenes / sensors +
//! an event broadcaster that drives the `/clip/v2/eventstream` SSE feed.
//!
//! Reference:
//! - Lights: developers.meethue.com/develop/hue-api/lights-api/ (v1 §1)
//!   and #resource_light_get (v2). v1 numeric "1", "2", ... IDs; v2 UUIDs.
//! - Groups: #4-groups-api (v1) and `room` / `zone` / `grouped_light` (v2).
//! - Scenes: #5-scenes-api (v1) and `scene` (v2).
//! - Sensors: #2-sensors-api (v1) and `motion` / `button` / `temperature` (v2).
//! - Eventstream: developers.meethue.com/develop/hue-api-v2/core-concepts/
//!   #eventstream — SSE payloads carry an array of `{id, type, data[]}`
//!   wrappers, where `type` is "add"/"update"/"delete".

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::broadcast;

/// One emulated light in storage. We keep it as a generic JSON-ish struct
/// so the v1 view (numeric id + `state` block) and the v2 view (UUID +
/// `on`/`dimming`/`color`/...) can be served from the same record without
/// double-storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatedLight {
    /// v1 numeric id (assigned by the registry).
    pub id_v1: String,
    /// v2 stable UUID.
    pub id_v2: uuid::Uuid,
    /// Display name. Per Charter v6 ADR-007 this is what the user sees as
    /// "Lamba" in the cave-home Portal.
    pub name: String,
    /// Manufacturer string for `lights/<id>.manufacturername` (v1) +
    /// `device.product_data.manufacturer_name` (v2). Default Signify.
    pub manufacturer_name: String,
    /// Hardware model id. e.g. "LCT012". Default = a Hue color candle clone.
    pub model_id: String,
    /// "Extended color light" / "Color light" / "Dimmable light".
    pub light_type: String,
    /// Is the light on?
    pub on: bool,
    /// Brightness percent — 0.0..=100.0 (v2) / 0..=254 (v1 = pct*2.54).
    pub brightness: f32,
    /// CIE xy — None for non-color lights.
    pub xy: Option<(f32, f32)>,
    /// Mirek color temperature (153..=500).
    pub mirek: Option<u16>,
}

impl EmulatedLight {
    /// Build a stock Hue color candle clone with a given name + v1 id.
    /// Random UUID, defaults match a published "Hue color candle" device.
    #[must_use]
    pub fn new_color_candle(name: impl Into<String>, id_v1: impl Into<String>) -> Self {
        Self {
            id_v1: id_v1.into(),
            id_v2: uuid::Uuid::new_v4(),
            name: name.into(),
            manufacturer_name: "Signify Netherlands B.V.".into(),
            model_id: "LCT012".into(),
            light_type: "Extended color light".into(),
            on: false,
            brightness: 100.0,
            xy: Some((0.4, 0.4)),
            mirek: Some(366),
        }
    }
    /// True iff this light supports color (xy set).
    #[must_use]
    pub const fn supports_color(&self) -> bool {
        self.xy.is_some()
    }
    /// True iff this light supports color temperature.
    #[must_use]
    pub const fn supports_color_temperature(&self) -> bool {
        self.mirek.is_some()
    }
}

/// One v1 group ("Room", "Zone", "LightGroup", "Luminaire"). Also stores
/// the v2 `room`/`zone` shape — same data, different projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatedGroup {
    pub id_v1: String,
    pub id_v2: uuid::Uuid,
    /// Type — "Room" / "Zone" / "LightGroup" / "Luminaire".
    pub group_type: String,
    /// Name — what the user sees as "Oda" in the Portal.
    pub name: String,
    /// Room archetype string (v2 only — informational in v1).
    pub archetype: String,
    /// Member light v1 IDs. The v2 view projects this through
    /// [`crate::api::v2`] using the light-id-v1 -> uuid map.
    pub member_lights_v1: Vec<String>,
}

impl EmulatedGroup {
    /// Build a stock living-room group.
    #[must_use]
    pub fn new_room(name: impl Into<String>, id_v1: impl Into<String>) -> Self {
        Self {
            id_v1: id_v1.into(),
            id_v2: uuid::Uuid::new_v4(),
            group_type: "Room".into(),
            name: name.into(),
            archetype: "living_room".into(),
            member_lights_v1: Vec::new(),
        }
    }
}

/// One scene. Stores the group it belongs to + per-light actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatedScene {
    pub id_v1: String,
    pub id_v2: uuid::Uuid,
    pub name: String,
    /// v1 group id this scene targets.
    pub group_v1: String,
    /// `light_v1 -> per-light action snapshot`.
    pub actions: BTreeMap<String, EmulatedSceneAction>,
}

/// Per-light snapshot inside a scene.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmulatedSceneAction {
    pub on: Option<bool>,
    pub brightness: Option<f32>,
    pub xy: Option<(f32, f32)>,
    pub mirek: Option<u16>,
}

/// One sensor (button / motion / temperature / daylight).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatedSensor {
    pub id_v1: String,
    pub id_v2: uuid::Uuid,
    pub name: String,
    pub sensor_type: String, // "ZLLSwitch", "ZLLPresence", "ZLLTemperature", ...
    pub state: Map<String, Value>,
    pub config: Map<String, Value>,
}

/// One event broadcast across the v2 eventstream. Reference:
/// developers.meethue.com/develop/hue-api-v2/core-concepts/#eventstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    pub id: String,
    pub creationtime: String,
    #[serde(rename = "type")]
    pub kind: StreamEventKind,
    pub data: Vec<Value>,
}

/// `add` / `update` / `delete` — the published v2 event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamEventKind {
    Add,
    Update,
    Delete,
}

/// The whole shared registry. Held inside the bridge process; cloned via
/// `Arc` to the HTTP handlers.
pub struct BridgeRegistry {
    lights: RwLock<BTreeMap<String, EmulatedLight>>,
    groups: RwLock<BTreeMap<String, EmulatedGroup>>,
    scenes: RwLock<BTreeMap<String, EmulatedScene>>,
    sensors: RwLock<BTreeMap<String, EmulatedSensor>>,
    next_light_id: RwLock<u32>,
    next_group_id: RwLock<u32>,
    next_scene_id: RwLock<u32>,
    next_sensor_id: RwLock<u32>,
    event_tx: broadcast::Sender<StreamEvent>,
}

impl Default for BridgeRegistry {
    fn default() -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            lights: RwLock::default(),
            groups: RwLock::default(),
            scenes: RwLock::default(),
            sensors: RwLock::default(),
            next_light_id: RwLock::new(1),
            next_group_id: RwLock::new(1),
            next_scene_id: RwLock::new(1),
            next_sensor_id: RwLock::new(1),
            event_tx,
        }
    }
}

impl BridgeRegistry {
    /// Convenience constructor.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    // ----- lights ----------------------------------------------------------

    /// Insert a light and return its v1 id.
    pub fn add_light(&self, mut light: EmulatedLight) -> String {
        if light.id_v1.is_empty() {
            let mut id = self.next_light_id.write();
            light.id_v1 = id.to_string();
            *id += 1;
        }
        let id_v1 = light.id_v1.clone();
        self.broadcast(StreamEventKind::Add, "light", &light.id_v2.to_string());
        self.lights.write().insert(id_v1.clone(), light);
        id_v1
    }

    /// Look up a light by v1 id.
    #[must_use]
    pub fn light(&self, id_v1: &str) -> Option<EmulatedLight> {
        self.lights.read().get(id_v1).cloned()
    }

    /// Look up a light by v2 UUID.
    #[must_use]
    pub fn light_by_uuid(&self, uuid: &uuid::Uuid) -> Option<EmulatedLight> {
        self.lights
            .read()
            .values()
            .find(|l| &l.id_v2 == uuid)
            .cloned()
    }

    /// List lights ordered by v1 id ascending.
    #[must_use]
    pub fn lights(&self) -> Vec<EmulatedLight> {
        self.lights.read().values().cloned().collect()
    }

    /// Apply a state update to a light by v1 id. Returns the new snapshot.
    pub fn update_light(
        &self,
        id_v1: &str,
        patch: impl FnOnce(&mut EmulatedLight),
    ) -> Option<EmulatedLight> {
        let mut guard = self.lights.write();
        let light = guard.get_mut(id_v1)?;
        patch(light);
        let snapshot = light.clone();
        drop(guard);
        self.broadcast(StreamEventKind::Update, "light", &snapshot.id_v2.to_string());
        Some(snapshot)
    }

    // ----- groups ----------------------------------------------------------

    pub fn add_group(&self, mut group: EmulatedGroup) -> String {
        if group.id_v1.is_empty() {
            let mut id = self.next_group_id.write();
            group.id_v1 = id.to_string();
            *id += 1;
        }
        let id_v1 = group.id_v1.clone();
        self.broadcast(StreamEventKind::Add, "room", &group.id_v2.to_string());
        self.groups.write().insert(id_v1.clone(), group);
        id_v1
    }

    #[must_use]
    pub fn group(&self, id_v1: &str) -> Option<EmulatedGroup> {
        self.groups.read().get(id_v1).cloned()
    }

    #[must_use]
    pub fn groups(&self) -> Vec<EmulatedGroup> {
        self.groups.read().values().cloned().collect()
    }

    // ----- scenes ----------------------------------------------------------

    pub fn add_scene(&self, mut scene: EmulatedScene) -> String {
        if scene.id_v1.is_empty() {
            let mut id = self.next_scene_id.write();
            scene.id_v1 = id.to_string();
            *id += 1;
        }
        let id_v1 = scene.id_v1.clone();
        self.broadcast(StreamEventKind::Add, "scene", &scene.id_v2.to_string());
        self.scenes.write().insert(id_v1.clone(), scene);
        id_v1
    }

    #[must_use]
    pub fn scene(&self, id_v1: &str) -> Option<EmulatedScene> {
        self.scenes.read().get(id_v1).cloned()
    }

    #[must_use]
    pub fn scenes(&self) -> Vec<EmulatedScene> {
        self.scenes.read().values().cloned().collect()
    }

    /// Activate a scene: copy each per-light action into the live light state.
    /// Returns the number of lights affected.
    pub fn recall_scene(&self, id_v1: &str) -> usize {
        let Some(scene) = self.scene(id_v1) else {
            return 0;
        };
        let mut affected = 0;
        for (light_id, action) in &scene.actions {
            let changed = self
                .update_light(light_id, |l| {
                    if let Some(v) = action.on {
                        l.on = v;
                    }
                    if let Some(v) = action.brightness {
                        l.brightness = v;
                    }
                    if action.xy.is_some() {
                        l.xy = action.xy;
                    }
                    if action.mirek.is_some() {
                        l.mirek = action.mirek;
                    }
                })
                .is_some();
            if changed {
                affected += 1;
            }
        }
        affected
    }

    // ----- sensors ---------------------------------------------------------

    pub fn add_sensor(&self, mut sensor: EmulatedSensor) -> String {
        if sensor.id_v1.is_empty() {
            let mut id = self.next_sensor_id.write();
            sensor.id_v1 = id.to_string();
            *id += 1;
        }
        let id_v1 = sensor.id_v1.clone();
        let kind = if sensor.sensor_type.contains("Switch") {
            "button"
        } else if sensor.sensor_type.contains("Presence") {
            "motion"
        } else {
            "temperature"
        };
        self.broadcast(StreamEventKind::Add, kind, &sensor.id_v2.to_string());
        self.sensors.write().insert(id_v1.clone(), sensor);
        id_v1
    }

    #[must_use]
    pub fn sensor(&self, id_v1: &str) -> Option<EmulatedSensor> {
        self.sensors.read().get(id_v1).cloned()
    }

    #[must_use]
    pub fn sensors(&self) -> Vec<EmulatedSensor> {
        self.sensors.read().values().cloned().collect()
    }

    // ----- events ----------------------------------------------------------

    /// Subscribe to the eventstream broadcaster. New subscribers receive
    /// events fired after they subscribe.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.event_tx.subscribe()
    }

    /// Fire one event. Returns the number of receivers it reached.
    pub fn broadcast(&self, kind: StreamEventKind, rtype: &str, rid: &str) -> usize {
        let event = StreamEvent {
            id: uuid::Uuid::new_v4().to_string(),
            creationtime: "1970-01-01T00:00:00Z".into(),
            kind,
            data: vec![serde_json::json!({"id": rid, "type": rtype})],
        };
        self.event_tx.send(event).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn add_light_assigns_sequential_v1_ids() {
        let reg = BridgeRegistry::new();
        let a = reg.add_light(EmulatedLight::new_color_candle("Mutfak", ""));
        let b = reg.add_light(EmulatedLight::new_color_candle("Salon", ""));
        assert_eq!(a, "1");
        assert_eq!(b, "2");
    }

    #[test]
    fn update_light_mutates_in_place_and_returns_snapshot() {
        let reg = BridgeRegistry::new();
        let id = reg.add_light(EmulatedLight::new_color_candle("Mutfak", ""));
        let snap = reg
            .update_light(&id, |l| {
                l.on = true;
                l.brightness = 50.0;
            })
            .unwrap();
        assert!(snap.on);
        assert!((snap.brightness - 50.0).abs() < 1e-3);
        let fresh = reg.light(&id).unwrap();
        assert!(fresh.on);
    }

    #[test]
    fn scene_recall_propagates_action_to_member_lights() {
        let reg = BridgeRegistry::new();
        let l1 = reg.add_light(EmulatedLight::new_color_candle("A", ""));
        let l2 = reg.add_light(EmulatedLight::new_color_candle("B", ""));
        let mut actions = BTreeMap::new();
        actions.insert(
            l1.clone(),
            EmulatedSceneAction {
                on: Some(true),
                brightness: Some(75.0),
                ..Default::default()
            },
        );
        actions.insert(
            l2.clone(),
            EmulatedSceneAction {
                on: Some(false),
                ..Default::default()
            },
        );
        let scene_id = reg.add_scene(EmulatedScene {
            id_v1: String::new(),
            id_v2: uuid::Uuid::new_v4(),
            name: "Aksam".into(),
            group_v1: "1".into(),
            actions,
        });
        let n = reg.recall_scene(&scene_id);
        assert_eq!(n, 2);
        assert!(reg.light(&l1).unwrap().on);
        assert!((reg.light(&l1).unwrap().brightness - 75.0).abs() < 1e-3);
        assert!(!reg.light(&l2).unwrap().on);
    }

    #[tokio::test]
    async fn add_light_fires_add_event_on_eventstream() {
        let reg = BridgeRegistry::new();
        let mut sub = reg.subscribe();
        let _ = reg.add_light(EmulatedLight::new_color_candle("X", ""));
        let event = tokio::time::timeout(Duration::from_millis(100), sub.recv())
            .await
            .expect("event must arrive within 100ms")
            .expect("event must be Ok");
        assert_eq!(event.kind, StreamEventKind::Add);
        assert_eq!(event.data[0].get("type").unwrap(), "light");
    }

    #[tokio::test]
    async fn update_light_fires_update_event() {
        let reg = BridgeRegistry::new();
        let id = reg.add_light(EmulatedLight::new_color_candle("X", ""));
        let mut sub = reg.subscribe();
        reg.update_light(&id, |l| {
            l.on = true;
        });
        let event = tokio::time::timeout(Duration::from_millis(100), sub.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, StreamEventKind::Update);
    }

    #[test]
    fn light_supports_capability_helpers() {
        let l = EmulatedLight::new_color_candle("X", "1");
        assert!(l.supports_color());
        assert!(l.supports_color_temperature());
    }

    #[test]
    fn group_member_lights_persist() {
        let reg = BridgeRegistry::new();
        let l1 = reg.add_light(EmulatedLight::new_color_candle("A", ""));
        let l2 = reg.add_light(EmulatedLight::new_color_candle("B", ""));
        let mut g = EmulatedGroup::new_room("Salon", "");
        g.member_lights_v1 = vec![l1, l2];
        let g_id = reg.add_group(g);
        assert_eq!(reg.group(&g_id).unwrap().member_lights_v1.len(), 2);
    }
}
