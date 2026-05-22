// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! v2 CLIP API — `/clip/v2/resource/...` endpoints + `/clip/v2/eventstream`.
//!
//! Reference: developers.meethue.com/develop/hue-api-v2/api-reference/
//! Every endpoint here is described in the published v2 reference.

pub mod eventstream;
pub mod views;

use crate::api::v1::V1Context;
use crate::errors::HueProtocolError;
use crate::registry::BridgeRegistry;
use serde_json::{Map, Value, json};
use std::sync::Arc;

/// The v2 response envelope. Reference: every CLIP endpoint returns
/// `{"errors": [{"description": "..."}], "data": [...]}`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Envelope {
    pub errors: Vec<EnvelopeError>,
    pub data: Vec<Value>,
}

/// One element of `errors[]`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvelopeError {
    pub description: String,
}

impl Envelope {
    /// Convenience: a successful response with this data array.
    #[must_use]
    pub fn ok(data: Vec<Value>) -> Self {
        Self {
            errors: Vec::new(),
            data,
        }
    }
    /// Convenience: an error envelope with `description`.
    #[must_use]
    pub fn error(description: impl Into<String>) -> Self {
        Self {
            errors: vec![EnvelopeError {
                description: description.into(),
            }],
            data: Vec::new(),
        }
    }
    /// Render to `serde_json::Value`.
    #[must_use]
    pub fn into_value(self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Header name that every v2 request must carry. Reference: dev-portal.
pub const APP_KEY_HEADER: &str = "hue-application-key";

/// Verify a request's `hue-application-key` header.
#[must_use]
pub fn check_app_key(ctx: &V1Context, app_key: Option<&str>) -> Result<(), Envelope> {
    match app_key {
        Some(key) if ctx.pairing.whitelist_get(key).is_some() => Ok(()),
        _ => Err(Envelope::error("unauthorized user")),
    }
}

/// `GET /clip/v2/resource/light`.
pub fn get_lights(ctx: &V1Context, app_key: Option<&str>) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let data = ctx
        .registry
        .lights()
        .into_iter()
        .map(|l| views::light_view(&l))
        .collect();
    Envelope::ok(data)
}

/// `GET /clip/v2/resource/light/<uuid>`.
pub fn get_light(ctx: &V1Context, app_key: Option<&str>, uuid: &str) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let parsed = match uuid::Uuid::parse_str(uuid) {
        Ok(u) => u,
        Err(_) => return Envelope::error(format!("invalid id: {uuid}")),
    };
    match ctx.registry.light_by_uuid(&parsed) {
        Some(l) => Envelope::ok(vec![views::light_view(&l)]),
        None => Envelope::error("not found"),
    }
}

/// `PUT /clip/v2/resource/light/<uuid>` — accepts the published LightPut body.
pub fn put_light(
    ctx: &V1Context,
    app_key: Option<&str>,
    uuid: &str,
    body: &Value,
) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let parsed = match uuid::Uuid::parse_str(uuid) {
        Ok(u) => u,
        Err(_) => return Envelope::error(format!("invalid id: {uuid}")),
    };
    let Some(light) = ctx.registry.light_by_uuid(&parsed) else {
        return Envelope::error("not found");
    };
    let id_v1 = light.id_v1.clone();
    ctx.registry.update_light(&id_v1, |l| {
        if let Some(on) = body
            .get("on")
            .and_then(|v| v.get("on"))
            .and_then(Value::as_bool)
        {
            l.on = on;
        }
        if let Some(bri) = body
            .get("dimming")
            .and_then(|v| v.get("brightness"))
            .and_then(Value::as_f64)
        {
            l.brightness = bri as f32;
        }
        if let Some(xy) = body
            .get("color")
            .and_then(|v| v.get("xy"))
            .and_then(Value::as_object)
        {
            let x = xy.get("x").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let y = xy.get("y").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            l.xy = Some((x, y));
        }
        if let Some(mirek) = body
            .get("color_temperature")
            .and_then(|v| v.get("mirek"))
            .and_then(Value::as_i64)
        {
            l.mirek = Some(mirek.clamp(153, 500) as u16);
        }
    });
    Envelope::ok(vec![json!({
        "rid": parsed.to_string(),
        "rtype": "light"
    })])
}

/// `GET /clip/v2/resource/scene`.
pub fn get_scenes(ctx: &V1Context, app_key: Option<&str>) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let data = ctx
        .registry
        .scenes()
        .into_iter()
        .map(|s| views::scene_view(&s, &ctx.registry))
        .collect();
    Envelope::ok(data)
}

/// `PUT /clip/v2/resource/scene/<uuid>` — supports `{recall: {action: "active"}}`.
pub fn put_scene(
    ctx: &V1Context,
    app_key: Option<&str>,
    uuid: &str,
    body: &Value,
) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let parsed = match uuid::Uuid::parse_str(uuid) {
        Ok(u) => u,
        Err(_) => return Envelope::error(format!("invalid id: {uuid}")),
    };
    // Map v2 UUID to v1 id by lookup.
    let scene_v1 = ctx
        .registry
        .scenes()
        .into_iter()
        .find(|s| s.id_v2 == parsed);
    let Some(scene) = scene_v1 else {
        return Envelope::error("scene not found");
    };
    let action = body
        .get("recall")
        .and_then(|v| v.get("action"))
        .and_then(Value::as_str);
    if let Some("active" | "static" | "dynamic_palette") = action {
        ctx.registry.recall_scene(&scene.id_v1);
    }
    Envelope::ok(vec![json!({
        "rid": parsed.to_string(),
        "rtype": "scene"
    })])
}

/// `GET /clip/v2/resource/room`.
pub fn get_rooms(ctx: &V1Context, app_key: Option<&str>) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let data = ctx
        .registry
        .groups()
        .into_iter()
        .filter(|g| g.group_type == "Room")
        .map(|g| views::room_or_zone_view(&g, &ctx.registry, "room"))
        .collect();
    Envelope::ok(data)
}

/// `GET /clip/v2/resource/zone`.
pub fn get_zones(ctx: &V1Context, app_key: Option<&str>) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let data = ctx
        .registry
        .groups()
        .into_iter()
        .filter(|g| g.group_type == "Zone")
        .map(|g| views::room_or_zone_view(&g, &ctx.registry, "zone"))
        .collect();
    Envelope::ok(data)
}

/// `GET /clip/v2/resource/bridge` — single-resource list.
pub fn get_bridge(ctx: &V1Context, app_key: Option<&str>) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    Envelope::ok(vec![views::bridge_view(&ctx.identity)])
}

/// `GET /clip/v2/resource` — *every* resource (devices, lights, scenes, rooms, ...).
/// Reference: dev-portal §"Retrieve all resources".
pub fn get_all_resources(ctx: &V1Context, app_key: Option<&str>) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let mut data: Vec<Value> = Vec::new();
    data.push(views::bridge_view(&ctx.identity));
    for light in ctx.registry.lights() {
        data.push(views::light_view(&light));
        data.push(views::device_view_for_light(&light));
    }
    for group in ctx.registry.groups() {
        let rtype = if group.group_type == "Zone" {
            "zone"
        } else {
            "room"
        };
        data.push(views::room_or_zone_view(&group, &ctx.registry, rtype));
        data.push(views::grouped_light_for_group(&group, &ctx.registry));
    }
    for scene in ctx.registry.scenes() {
        data.push(views::scene_view(&scene, &ctx.registry));
    }
    Envelope::ok(data)
}

/// Helper for tests + future routing: list the resource types we serve.
#[must_use]
pub fn supported_resource_segments() -> &'static [&'static str] {
    &[
        "light",
        "scene",
        "room",
        "zone",
        "grouped_light",
        "bridge",
        "device",
        "motion",
        "button",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BridgeIdentity;
    use crate::pairing::PairingService;
    use crate::registry::{BridgeRegistry, EmulatedLight};

    fn paired_ctx() -> (V1Context, String) {
        let identity = BridgeIdentity::fresh("10.0.0.5");
        let pairing = PairingService::new();
        let registry = BridgeRegistry::new();
        pairing.begin_link_window();
        let key = pairing
            .try_pair(
                &crate::pairing::PairRequest {
                    devicetype: "cave-home#test".into(),
                    generateclientkey: false,
                },
                "t",
            )
            .unwrap()
            .username;
        (
            V1Context {
                identity,
                pairing,
                registry,
            },
            key,
        )
    }

    #[test]
    fn get_lights_unauthorized_without_app_key() {
        let (ctx, _) = paired_ctx();
        let env = get_lights(&ctx, None);
        assert_eq!(env.errors.len(), 1);
        assert!(env.data.is_empty());
    }

    #[test]
    fn get_lights_returns_data_when_authorised() {
        let (ctx, key) = paired_ctx();
        let _ = ctx
            .registry
            .add_light(EmulatedLight::new_color_candle("L", ""));
        let env = get_lights(&ctx, Some(&key));
        assert!(env.errors.is_empty());
        assert_eq!(env.data.len(), 1);
        let l = &env.data[0];
        assert_eq!(l.get("type").unwrap(), &Value::from("light"));
    }

    #[test]
    fn put_light_applies_on_and_dimming_and_returns_rid() {
        let (ctx, key) = paired_ctx();
        let id = ctx
            .registry
            .add_light(EmulatedLight::new_color_candle("L", ""));
        let light = ctx.registry.light(&id).unwrap();
        let env = put_light(
            &ctx,
            Some(&key),
            &light.id_v2.to_string(),
            &json!({"on": {"on": true}, "dimming": {"brightness": 33.0}}),
        );
        assert!(env.errors.is_empty());
        assert_eq!(env.data.len(), 1);
        let rid = env.data[0].get("rid").unwrap().as_str().unwrap();
        assert_eq!(rid, light.id_v2.to_string());
        let updated = ctx.registry.light(&id).unwrap();
        assert!(updated.on);
        assert!((updated.brightness - 33.0).abs() < 1e-3);
    }

    #[test]
    fn put_scene_active_recalls_scene() {
        let (ctx, key) = paired_ctx();
        let l_id = ctx
            .registry
            .add_light(EmulatedLight::new_color_candle("L", ""));
        let mut actions = std::collections::BTreeMap::new();
        actions.insert(
            l_id.clone(),
            crate::registry::EmulatedSceneAction {
                on: Some(true),
                brightness: Some(80.0),
                ..Default::default()
            },
        );
        let scene_uuid;
        {
            let s = crate::registry::EmulatedScene {
                id_v1: String::new(),
                id_v2: uuid::Uuid::new_v4(),
                name: "Aksam".into(),
                group_v1: "1".into(),
                actions,
            };
            scene_uuid = s.id_v2;
            ctx.registry.add_scene(s);
        }
        let env = put_scene(
            &ctx,
            Some(&key),
            &scene_uuid.to_string(),
            &json!({"recall": {"action": "active"}}),
        );
        assert!(env.errors.is_empty(), "envelope errors: {:?}", env.errors);
        assert!(ctx.registry.light(&l_id).unwrap().on);
    }

    #[test]
    fn get_all_resources_includes_bridge_lights_rooms_scenes() {
        let (ctx, key) = paired_ctx();
        ctx.registry
            .add_light(EmulatedLight::new_color_candle("L", ""));
        ctx.registry
            .add_group(crate::registry::EmulatedGroup::new_room("R", ""));
        ctx.registry.add_scene(crate::registry::EmulatedScene {
            id_v1: String::new(),
            id_v2: uuid::Uuid::new_v4(),
            name: "S".into(),
            group_v1: "1".into(),
            actions: std::collections::BTreeMap::new(),
        });
        let env = get_all_resources(&ctx, Some(&key));
        let types: std::collections::HashSet<&str> = env
            .data
            .iter()
            .filter_map(|v| v.get("type").and_then(Value::as_str))
            .collect();
        for needle in ["bridge", "light", "device", "room", "grouped_light", "scene"] {
            assert!(types.contains(needle), "missing type: {needle}");
        }
    }
}

// keep the unused-import warning at bay (BridgeRegistry is used only via fields).
#[allow(dead_code)]
fn _ensure_arc_brigde_registry_used(_: Arc<BridgeRegistry>) {}
#[allow(dead_code)]
fn _ensure_map_used(_: Map<String, Value>) {}
#[allow(dead_code)]
fn _ensure_protocol_error_used(_: HueProtocolError) {}
