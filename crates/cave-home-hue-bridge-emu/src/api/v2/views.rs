// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! v2 CLIP JSON view rendering.
//!
//! Reference: developer-portal `clip-api.schema.json` definitions + the
//! per-resource pages (`/develop/hue-api-v2/api-reference/#resource_*`).

use crate::config::BridgeIdentity;
use crate::registry::{BridgeRegistry, EmulatedGroup, EmulatedLight, EmulatedScene};
use serde_json::{Value, json};

/// Render a [`EmulatedLight`] as a v2 `light` resource.
/// Reference: `#resource_light_get`.
#[must_use]
pub fn light_view(light: &EmulatedLight) -> Value {
    let mut obj = json!({
        "id": light.id_v2.to_string(),
        "id_v1": format!("/lights/{}", light.id_v1),
        "owner": {
            "rid": light.id_v2.to_string(),
            "rtype": "device"
        },
        "metadata": {
            "name": light.name,
            "archetype": "candle_bulb"
        },
        "on": {"on": light.on},
        "dimming": {
            "brightness": light.brightness,
            "min_dim_level": 0.5
        },
        "mode": "normal",
        "type": "light"
    });
    if light.supports_color_temperature() {
        obj["color_temperature"] = json!({
            "mirek": light.mirek,
            "mirek_valid": true,
            "mirek_schema": {"mirek_minimum": 153, "mirek_maximum": 500}
        });
    }
    if light.supports_color() {
        let (x, y) = light.xy.unwrap_or((0.0, 0.0));
        obj["color"] = json!({
            "xy": {"x": x, "y": y},
            "gamut": {
                "red": {"x": 0.6915, "y": 0.3083},
                "green": {"x": 0.17, "y": 0.7},
                "blue": {"x": 0.1532, "y": 0.0475}
            },
            "gamut_type": "C"
        });
    }
    obj
}

/// Render the synthetic "device" wrapping a single light.
#[must_use]
pub fn device_view_for_light(light: &EmulatedLight) -> Value {
    json!({
        "id": light.id_v2.to_string(),
        "id_v1": format!("/lights/{}", light.id_v1),
        "product_data": {
            "model_id": light.model_id,
            "manufacturer_name": light.manufacturer_name,
            "product_name": "Hue color candle",
            "product_archetype": "candle_bulb",
            "certified": true,
            "software_version": "1.108.10"
        },
        "metadata": {"name": light.name, "archetype": "candle_bulb"},
        "services": [
            {"rid": light.id_v2.to_string(), "rtype": "light"}
        ],
        "type": "device"
    })
}

/// Render a group as a `room` or `zone` v2 resource.
#[must_use]
pub fn room_or_zone_view(
    group: &EmulatedGroup,
    registry: &BridgeRegistry,
    rtype: &str,
) -> Value {
    let children: Vec<Value> = group
        .member_lights_v1
        .iter()
        .filter_map(|id| registry.light(id))
        .map(|l| {
            json!({
                "rid": l.id_v2.to_string(),
                "rtype": if rtype == "zone" { "light" } else { "device" }
            })
        })
        .collect();
    json!({
        "id": group.id_v2.to_string(),
        "id_v1": format!("/groups/{}", group.id_v1),
        "metadata": {
            "name": group.name,
            "archetype": group.archetype
        },
        "children": children,
        "services": [
            {
                "rid": group.id_v2.to_string(),
                "rtype": "grouped_light"
            }
        ],
        "type": rtype
    })
}

/// Render the auto-derived `grouped_light` resource that backs a room/zone.
#[must_use]
pub fn grouped_light_for_group(group: &EmulatedGroup, registry: &BridgeRegistry) -> Value {
    let any_on = group
        .member_lights_v1
        .iter()
        .filter_map(|id| registry.light(id))
        .any(|l| l.on);
    json!({
        "id": group.id_v2.to_string(),
        "id_v1": format!("/groups/{}", group.id_v1),
        "owner": {
            "rid": group.id_v2.to_string(),
            "rtype": if group.group_type == "Zone" { "zone" } else { "room" }
        },
        "on": {"on": any_on},
        "type": "grouped_light"
    })
}

/// Render a scene as a v2 `scene` resource.
#[must_use]
pub fn scene_view(scene: &EmulatedScene, registry: &BridgeRegistry) -> Value {
    let group_uuid = registry
        .group(&scene.group_v1)
        .map(|g| g.id_v2.to_string())
        .unwrap_or_default();
    let actions: Vec<Value> = scene
        .actions
        .iter()
        .filter_map(|(light_v1, action)| {
            let light = registry.light(light_v1)?;
            let mut payload = serde_json::Map::new();
            if let Some(on) = action.on {
                payload.insert("on".into(), json!({"on": on}));
            }
            if let Some(b) = action.brightness {
                payload.insert("dimming".into(), json!({"brightness": b}));
            }
            if let Some((x, y)) = action.xy {
                payload.insert("color".into(), json!({"xy": {"x": x, "y": y}}));
            }
            if let Some(mirek) = action.mirek {
                payload.insert(
                    "color_temperature".into(),
                    json!({"mirek": mirek}),
                );
            }
            Some(json!({
                "target": {"rid": light.id_v2.to_string(), "rtype": "light"},
                "action": Value::Object(payload)
            }))
        })
        .collect();
    json!({
        "id": scene.id_v2.to_string(),
        "id_v1": format!("/scenes/{}", scene.id_v1),
        "metadata": {"name": scene.name},
        "group": {"rid": group_uuid, "rtype": "room"},
        "actions": actions,
        "type": "scene"
    })
}

/// Render the singleton `bridge` resource at `/clip/v2/resource/bridge`.
#[must_use]
pub fn bridge_view(identity: &BridgeIdentity) -> Value {
    json!({
        "id": identity.uuid.to_string(),
        "owner": {"rid": identity.uuid.to_string(), "rtype": "device"},
        "bridge_id": identity.bridge_id,
        "time_zone": {"time_zone": "Europe/Istanbul"},
        "type": "bridge"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::EmulatedLight;

    #[test]
    fn light_view_includes_dimming_and_color() {
        let l = EmulatedLight::new_color_candle("X", "1");
        let v = light_view(&l);
        assert_eq!(v.get("type").unwrap(), &Value::from("light"));
        assert!(v.get("dimming").is_some());
        assert!(v.get("color").is_some());
        assert!(v.get("color_temperature").is_some());
        let metadata = v.get("metadata").unwrap();
        assert_eq!(metadata.get("name").unwrap(), &Value::from("X"));
    }

    #[test]
    fn bridge_view_includes_bridge_id_and_uuid() {
        let id = BridgeIdentity::fresh("10.0.0.7");
        let v = bridge_view(&id);
        assert_eq!(v.get("type").unwrap(), &Value::from("bridge"));
        assert!(v.get("id").is_some());
        assert_eq!(
            v.get("bridge_id").unwrap().as_str().unwrap(),
            id.bridge_id.as_str()
        );
    }

    #[test]
    fn light_without_color_skips_color_field() {
        let mut l = EmulatedLight::new_color_candle("X", "1");
        l.xy = None;
        let v = light_view(&l);
        assert!(v.get("color").is_none());
        // Still has color temperature because we left mirek set.
        assert!(v.get("color_temperature").is_some());
    }
}
