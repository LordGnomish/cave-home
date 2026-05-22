// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/models/scene.py
//! v2 Scene resource. Mirrors `aiohue.v2.models.scene`.
//!
//! Reference: <https://developers.meethue.com/develop/hue-api-v2/api-reference/#resource_scene>

use crate::v2::models::feature::{
    ColorFeatureBase, ColorTemperatureFeatureBase, DimmingFeatureBase, OnFeature,
};
use crate::v2::models::resource::{ResourceIdentifier, ResourceTypes};
use serde::{Deserialize, Serialize};

/// `aiohue.v2.models.scene.SceneMetadata`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ResourceIdentifier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appdata: Option<String>,
}

/// `aiohue.v2.models.scene.SceneAction` — one light's actions inside a scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<OnFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimming: Option<DimmingFeatureBase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorFeatureBase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_temperature: Option<ColorTemperatureFeatureBase>,
}

/// `aiohue.v2.models.scene.SceneActionElement` — pairs a target light with its action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneActionElement {
    pub target: ResourceIdentifier,
    pub action: SceneAction,
}

/// `aiohue.v2.models.scene.SceneRecallAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneRecallAction {
    Active,
    DynamicPalette,
    Static,
    Deactivate,
}

/// `aiohue.v2.models.scene.SceneRecall` — PUT body to recall a scene.
#[derive(Debug, Default, Clone, Serialize)]
pub struct SceneRecall {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<SceneRecallAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimming: Option<DimmingFeatureBase>,
}

/// `aiohue.v2.models.scene.Scene`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_v1: Option<String>,
    pub metadata: SceneMetadata,
    pub group: ResourceIdentifier,
    pub actions: Vec<SceneActionElement>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<ResourceTypes>,
}

/// `aiohue.v2.models.scene.ScenePut` — PUT body for `/clip/v2/resource/scene/{id}`.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ScenePut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SceneMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recall: Option<SceneRecall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<SceneActionElement>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scene_decodes_and_recall_round_trips() {
        let scene_json = json!({
            "id": "scene-1",
            "metadata": {"name": "Aksam"},
            "group": {"rid": "room-1", "rtype": "room"},
            "actions": [
                {
                    "target": {"rid": "light-1", "rtype": "light"},
                    "action": {"on": {"on": true}, "dimming": {"brightness": 50.0}}
                }
            ]
        });
        let s: Scene = serde_json::from_value(scene_json).unwrap();
        assert_eq!(s.metadata.name, "Aksam");
        assert_eq!(s.actions.len(), 1);
        assert!(s.actions[0].action.dimming.is_some());

        let put = ScenePut {
            recall: Some(SceneRecall {
                action: Some(SceneRecallAction::Active),
                duration: Some(500),
                ..Default::default()
            }),
            ..Default::default()
        };
        let body = serde_json::to_string(&put).unwrap();
        assert!(body.contains("\"action\":\"active\""));
        assert!(body.contains("\"duration\":500"));
    }
}
