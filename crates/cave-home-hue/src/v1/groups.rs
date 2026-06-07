// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v1/groups.py
//! v1 groups controller — Hue "Room" / "Zone" / "Luminaire" / "LightGroup".
//!
//! Reference: <https://developers.meethue.com/documentation/groups-api>.

use crate::errors::HueResult;
use crate::v1::api::{ApiItems, RawItem, V1Item, V1Request};
use serde::Serialize;
use serde_json::{Value, json};

/// `aiohue.v1.groups.GroupState` — readonly aggregate state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupState {
    pub all_on: bool,
    pub any_on: bool,
}

/// `aiohue.v1.groups.GroupAction` — pending action template (last applied or queued).
#[derive(Debug, Default, Clone, Serialize)]
pub struct GroupAction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bri: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hue: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sat: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xy: Option<(f32, f32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ct: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
    #[serde(rename = "transitiontime", skip_serializing_if = "Option::is_none")]
    pub transition_time: Option<u16>,
    #[serde(rename = "bri_inc", skip_serializing_if = "Option::is_none")]
    pub bri_inc: Option<i16>,
    #[serde(rename = "sat_inc", skip_serializing_if = "Option::is_none")]
    pub sat_inc: Option<i16>,
    #[serde(rename = "hue_inc", skip_serializing_if = "Option::is_none")]
    pub hue_inc: Option<i32>,
    #[serde(rename = "ct_inc", skip_serializing_if = "Option::is_none")]
    pub ct_inc: Option<i16>,
    #[serde(rename = "xy_inc", skip_serializing_if = "Option::is_none")]
    pub xy_inc: Option<(f32, f32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
}

/// `aiohue.v1.groups.Group`.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: String,
    pub raw: RawItem,
}

impl V1Item for Group {
    const ITEM_TYPE: &'static str = "groups";
    fn from_raw(id: String, raw: RawItem) -> Self {
        Self { id, raw }
    }
    fn set_raw(&mut self, raw: RawItem) {
        self.raw = raw;
    }
}

impl Group {
    /// `aiohue.v1.groups.Group.type` — "Room" / "Zone" / "Luminaire" / ...
    #[must_use]
    pub fn group_type(&self) -> Option<&str> {
        self.raw.get("type").and_then(Value::as_str)
    }
    /// `aiohue.v1.groups.Group.name`.
    #[must_use]
    pub fn name(&self) -> &str {
        self.raw.get("name").and_then(Value::as_str).unwrap_or("")
    }
    /// `aiohue.v1.groups.Group.uniqueid` — for Luminaire/Lightsource groups
    /// only (API ≥ 1.9).
    #[must_use]
    pub fn unique_id(&self) -> Option<&str> {
        self.raw.get("uniqueid").and_then(Value::as_str)
    }
    /// `aiohue.v1.groups.Group.lights` — IDs of member lights.
    #[must_use]
    pub fn lights(&self) -> Vec<String> {
        self.raw
            .get("lights")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// `aiohue.v1.groups.Group.sensors` — IDs of member sensors.
    #[must_use]
    pub fn sensors(&self) -> Vec<String> {
        self.raw
            .get("sensors")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// `aiohue.v1.groups.Group.state`.
    #[must_use]
    pub fn state(&self) -> Option<GroupState> {
        let s = self.raw.get("state")?.as_object()?;
        Some(GroupState {
            all_on: s.get("all_on")?.as_bool()?,
            any_on: s.get("any_on")?.as_bool()?,
        })
    }
    /// `aiohue.v1.groups.Group.set_action` — apply a pending action.
    pub async fn set_action(
        &self,
        req: &dyn V1Request,
        action: &GroupAction,
    ) -> HueResult<()> {
        let body = serde_json::to_value(action).unwrap_or(json!({}));
        let _ = req
            .put(&format!("groups/{}/action", self.id), body)
            .await?;
        Ok(())
    }
}

/// `aiohue.v1.groups.Groups`.
pub type Groups = ApiItems<Group>;

/// `aiohue.v1.groups.Groups.__init__`.
#[must_use]
pub fn new_groups() -> Groups {
    ApiItems::new("groups")
}

/// `aiohue.v1.groups.Groups.get_all_lights_group` — fetches group `0`,
/// which is the implicit "all-lights" set.
pub async fn get_all_lights_group(req: &dyn V1Request) -> HueResult<Group> {
    let raw = req.get("groups/0").await?;
    let map = raw
        .as_object()
        .cloned()
        .unwrap_or_default();
    Ok(Group::from_raw("0".into(), map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[test]
    fn group_exposes_lights_and_sensors() {
        let raw = json!({
            "type": "Room",
            "name": "Salon",
            "lights": ["1", "2", "3"],
            "sensors": ["s1"],
            "state": {"all_on": false, "any_on": true},
        });
        let g = Group::from_raw("4".into(), raw.as_object().unwrap().clone());
        assert_eq!(g.group_type(), Some("Room"));
        assert_eq!(g.name(), "Salon");
        assert_eq!(g.lights(), vec!["1", "2", "3"]);
        assert_eq!(g.sensors(), vec!["s1"]);
        assert_eq!(g.state(), Some(GroupState { all_on: false, any_on: true }));
    }

    #[test]
    fn group_action_with_scene_serialises() {
        let act = GroupAction {
            on: Some(true),
            scene: Some("Bxx0x".into()),
            ..GroupAction::default()
        };
        let body = serde_json::to_value(&act).unwrap();
        let obj = body.as_object().unwrap();
        assert_eq!(obj.len(), 2);
        assert_eq!(obj.get("scene").unwrap(), &Value::String("Bxx0x".into()));
    }

    struct StubReq {
        called: Mutex<Vec<(String, String)>>, // (verb, path)
    }

    #[async_trait]
    impl V1Request for StubReq {
        async fn get(&self, path: &str) -> HueResult<Value> {
            self.called
                .lock()
                .unwrap()
                .push(("get".into(), path.into()));
            Ok(json!({"name": "All", "state": {"all_on": false, "any_on": false}, "lights": [], "sensors": []}))
        }
        async fn put(&self, path: &str, _body: Value) -> HueResult<Value> {
            self.called
                .lock()
                .unwrap()
                .push(("put".into(), path.into()));
            Ok(Value::Null)
        }
        async fn post(&self, path: &str, _body: Value) -> HueResult<Value> {
            self.called
                .lock()
                .unwrap()
                .push(("post".into(), path.into()));
            Ok(Value::Null)
        }
        async fn delete(&self, path: &str) -> HueResult<Value> {
            self.called
                .lock()
                .unwrap()
                .push(("delete".into(), path.into()));
            Ok(Value::Null)
        }
    }

    #[tokio::test]
    async fn get_all_lights_group_uses_path_groups_zero() {
        let req = StubReq {
            called: Mutex::new(Vec::new()),
        };
        let _ = get_all_lights_group(&req).await.unwrap();
        let calls = req.called.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], ("get".into(), "groups/0".into()));
    }

    #[tokio::test]
    async fn set_action_puts_to_groups_id_action() {
        let req = StubReq {
            called: Mutex::new(Vec::new()),
        };
        let g = Group::from_raw("5".into(), serde_json::Map::new());
        g.set_action(
            &req,
            &GroupAction {
                on: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let calls = req.called.lock().unwrap().clone();
        assert_eq!(calls[0], ("put".into(), "groups/5/action".into()));
    }
}
