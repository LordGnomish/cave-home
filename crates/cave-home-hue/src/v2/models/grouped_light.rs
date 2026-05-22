// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/models/grouped_light.py
//! v2 `grouped_light` — the resource that backs "control all lights in a
//! room / zone" in the v2 API. Mirrors `aiohue.v2.models.grouped_light`.

use crate::v2::models::feature::{
    AlertFeature, ColorFeatureBase, ColorTemperatureFeatureBase, DimmingFeatureBase, OnFeature,
};
use crate::v2::models::resource::{ResourceIdentifier, ResourceTypes};
use serde::{Deserialize, Serialize};

/// `aiohue.v2.models.grouped_light.GroupedLight`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupedLight {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_v1: Option<String>,
    pub owner: ResourceIdentifier,
    pub on: OnFeature,
    #[serde(default = "default_grouped_light_type", rename = "type")]
    pub type_: ResourceTypes,
}

const fn default_grouped_light_type() -> ResourceTypes {
    ResourceTypes::GroupedLight
}

/// `aiohue.v2.models.grouped_light.GroupedLightPut` — PUT body.
#[derive(Debug, Default, Clone, Serialize)]
pub struct GroupedLightPut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<OnFeature>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimming: Option<DimmingFeatureBase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_temperature: Option<ColorTemperatureFeatureBase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorFeatureBase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert: Option<AlertFeature>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn grouped_light_decodes() {
        let g: GroupedLight = serde_json::from_value(json!({
            "id": "grouped-1",
            "owner": {"rid": "room-1", "rtype": "room"},
            "on": {"on": true},
            "type": "grouped_light"
        }))
        .unwrap();
        assert!(g.on.on);
    }

    #[test]
    fn grouped_light_put_omits_unset_fields() {
        let put = GroupedLightPut {
            on: Some(OnFeature { on: false }),
            ..Default::default()
        };
        let s = serde_json::to_string(&put).unwrap();
        assert_eq!(s, "{\"on\":{\"on\":false}}");
    }
}
