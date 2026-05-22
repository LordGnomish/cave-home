// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v1/scenes.py
//! v1 scenes controller. Ports `aiohue.v1.scenes` line-by-line.

use crate::v1::api::{ApiItems, RawItem, V1Item};
use serde_json::Value;

/// `aiohue.v1.scenes.Scene` — a saved snapshot of light state across a
/// group, recallable via `groups/{id}/action {"scene": "<id>"}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub id: String,
    pub raw: RawItem,
}

impl V1Item for Scene {
    const ITEM_TYPE: &'static str = "scenes";
    fn from_raw(id: String, raw: RawItem) -> Self {
        Self { id, raw }
    }
    fn set_raw(&mut self, raw: RawItem) {
        self.raw = raw;
    }
}

impl Scene {
    /// `aiohue.v1.scenes.Scene.name`.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.raw.get("name").and_then(Value::as_str)
    }
    /// `aiohue.v1.scenes.Scene.lights` — member light IDs.
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
    /// `aiohue.v1.scenes.Scene.owner` — application key that authored the scene.
    #[must_use]
    pub fn owner(&self) -> Option<&str> {
        self.raw.get("owner").and_then(Value::as_str)
    }
    /// `aiohue.v1.scenes.Scene.recycle`.
    #[must_use]
    pub fn recycle(&self) -> Option<bool> {
        self.raw.get("recycle").and_then(Value::as_bool)
    }
    /// `aiohue.v1.scenes.Scene.locked`.
    #[must_use]
    pub fn locked(&self) -> Option<bool> {
        self.raw.get("locked").and_then(Value::as_bool)
    }
    /// `aiohue.v1.scenes.Scene.appdata` — opaque app-specific blob.
    #[must_use]
    pub fn appdata(&self) -> Option<&Value> {
        self.raw.get("appdata")
    }
    /// `aiohue.v1.scenes.Scene.picture` — picture id, may be empty.
    #[must_use]
    pub fn picture(&self) -> Option<&str> {
        self.raw.get("picture").and_then(Value::as_str)
    }
    /// `aiohue.v1.scenes.Scene.lastupdated` — ISO timestamp string.
    #[must_use]
    pub fn last_updated(&self) -> Option<&str> {
        self.raw.get("lastupdated").and_then(Value::as_str)
    }
    /// `aiohue.v1.scenes.Scene.version`.
    #[must_use]
    pub fn version(&self) -> Option<i64> {
        self.raw.get("version").and_then(Value::as_i64)
    }
}

/// `aiohue.v1.scenes.Scenes`.
pub type Scenes = ApiItems<Scene>;

#[must_use]
pub fn new_scenes() -> Scenes {
    ApiItems::new("scenes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scene_exposes_upstream_fields() {
        let raw = json!({
            "name": "Aksam",
            "lights": ["1", "2"],
            "owner": "abcdef0123456789abcdef0123456789abcdef01",
            "recycle": false,
            "locked": true,
            "appdata": {"data": "AUUVRD_r99_d99"},
            "picture": "",
            "lastupdated": "2026-05-17T20:00:00",
            "version": 2,
        });
        let scene = Scene::from_raw("aaa".into(), raw.as_object().unwrap().clone());
        assert_eq!(scene.name(), Some("Aksam"));
        assert_eq!(scene.lights(), vec!["1", "2"]);
        assert_eq!(scene.recycle(), Some(false));
        assert_eq!(scene.locked(), Some(true));
        assert_eq!(scene.last_updated(), Some("2026-05-17T20:00:00"));
        assert_eq!(scene.version(), Some(2));
    }
}
