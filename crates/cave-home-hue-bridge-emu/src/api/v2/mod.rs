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
use crate::num::{f64_to_f32, i64_to_u16};
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
    pub const fn ok(data: Vec<Value>) -> Self {
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
pub fn check_app_key(ctx: &V1Context, app_key: Option<&str>) -> Result<(), Envelope> {
    match app_key {
        Some(key) if ctx.pairing.whitelist_get(key).is_some() => Ok(()),
        _ => Err(Envelope::error("unauthorized user")),
    }
}

/// `GET /clip/v2/resource/light`.
#[must_use]
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
#[must_use]
pub fn get_light(ctx: &V1Context, app_key: Option<&str>, uuid: &str) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let Ok(parsed) = uuid::Uuid::parse_str(uuid) else {
        return Envelope::error(format!("invalid id: {uuid}"));
    };
    ctx.registry.light_by_uuid(&parsed).map_or_else(
        || Envelope::error("not found"),
        |l| Envelope::ok(vec![views::light_view(&l)]),
    )
}

/// `PUT /clip/v2/resource/light/<uuid>` — accepts the published `LightPut` body.
#[must_use]
pub fn put_light(ctx: &V1Context, app_key: Option<&str>, uuid: &str, body: &Value) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let Ok(parsed) = uuid::Uuid::parse_str(uuid) else {
        return Envelope::error(format!("invalid id: {uuid}"));
    };
    let Some(light) = ctx.registry.light_by_uuid(&parsed) else {
        return Envelope::error("not found");
    };
    let id_v1 = light.id_v1;
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
            l.brightness = f64_to_f32(bri);
        }
        if let Some(xy) = body
            .get("color")
            .and_then(|v| v.get("xy"))
            .and_then(Value::as_object)
        {
            let x = f64_to_f32(xy.get("x").and_then(Value::as_f64).unwrap_or(0.0));
            let y = f64_to_f32(xy.get("y").and_then(Value::as_f64).unwrap_or(0.0));
            l.xy = Some((x, y));
        }
        if let Some(mirek) = body
            .get("color_temperature")
            .and_then(|v| v.get("mirek"))
            .and_then(Value::as_i64)
        {
            l.mirek = Some(i64_to_u16(mirek.clamp(153, 500)));
        }
    });
    Envelope::ok(vec![json!({
        "rid": parsed.to_string(),
        "rtype": "light"
    })])
}

/// `GET /clip/v2/resource/scene`.
#[must_use]
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
pub fn put_scene(ctx: &V1Context, app_key: Option<&str>, uuid: &str, body: &Value) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    let Ok(parsed) = uuid::Uuid::parse_str(uuid) else {
        return Envelope::error(format!("invalid id: {uuid}"));
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
#[must_use]
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
#[must_use]
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
#[must_use]
pub fn get_bridge(ctx: &V1Context, app_key: Option<&str>) -> Envelope {
    if let Err(e) = check_app_key(ctx, app_key) {
        return e;
    }
    Envelope::ok(vec![views::bridge_view(&ctx.identity)])
}

/// `GET /clip/v2/resource` — *every* resource (devices, lights, scenes, rooms, ...).
/// Reference: dev-portal §"Retrieve all resources".
#[must_use]
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
pub const fn supported_resource_segments() -> &'static [&'static str] {
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

// keep the unused-import warning at bay (BridgeRegistry is used only via fields).
#[allow(dead_code)]
fn _ensure_arc_brigde_registry_used(_: Arc<BridgeRegistry>) {}
#[allow(dead_code)]
fn _ensure_map_used(_: Map<String, Value>) {}
#[allow(dead_code)]
fn _ensure_protocol_error_used(_: HueProtocolError) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BridgeIdentity;
    use crate::errors::EmuError;
    use crate::pairing::PairingService;
    use crate::registry::{BridgeRegistry, EmulatedLight};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn paired_ctx() -> Result<(V1Context, String), EmuError> {
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
            )?
            .username;
        Ok((
            V1Context {
                identity,
                pairing,
                registry,
            },
            key,
        ))
    }

    #[test]
    fn get_lights_unauthorized_without_app_key() -> TestResult {
        let (ctx, _) = paired_ctx()?;
        let env = get_lights(&ctx, None);
        assert_eq!(env.errors.len(), 1);
        assert_eq!(env.data, [] as [Value; 0]);
        Ok(())
    }

    #[test]
    fn get_lights_returns_data_when_authorised() -> TestResult {
        let (ctx, key) = paired_ctx()?;
        let _ = ctx
            .registry
            .add_light(EmulatedLight::new_color_candle("L", ""));
        let env = get_lights(&ctx, Some(&key));
        assert!(env.errors.is_empty());
        assert_eq!(env.data.len(), 1);
        let l = &env.data[0];
        assert_eq!(
            l.get("type").ok_or("expected Some, got None")?,
            &Value::from("light")
        );
        Ok(())
    }

    #[test]
    fn put_light_applies_on_and_dimming_and_returns_rid() -> TestResult {
        let (ctx, key) = paired_ctx()?;
        let id = ctx
            .registry
            .add_light(EmulatedLight::new_color_candle("L", ""));
        let light = ctx.registry.light(&id).ok_or("expected Some, got None")?;
        let env = put_light(
            &ctx,
            Some(&key),
            &light.id_v2.to_string(),
            &json!({"on": {"on": true}, "dimming": {"brightness": 33.0}}),
        );
        assert!(env.errors.is_empty());
        assert_eq!(env.data.len(), 1);
        let rid = env.data[0]
            .get("rid")
            .ok_or("expected Some, got None")?
            .as_str()
            .ok_or("expected Some, got None")?;
        assert_eq!(rid, light.id_v2.to_string());
        let updated = ctx.registry.light(&id).ok_or("expected Some, got None")?;
        assert!(updated.on);
        assert!((updated.brightness - 33.0).abs() < 1e-3);
        Ok(())
    }

    #[test]
    fn put_scene_active_recalls_scene() -> TestResult {
        let (ctx, key) = paired_ctx()?;
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
        assert!(
            ctx.registry
                .light(&l_id)
                .ok_or("expected Some, got None")?
                .on
        );
        Ok(())
    }

    #[test]
    fn get_all_resources_includes_bridge_lights_rooms_scenes() -> TestResult {
        let (ctx, key) = paired_ctx()?;
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
        for needle in [
            "bridge",
            "light",
            "device",
            "room",
            "grouped_light",
            "scene",
        ] {
            assert!(types.contains(needle), "missing type: {needle}");
        }
        Ok(())
    }

    #[test]
    fn put_light_edge_values() -> TestResult {
        let (ctx, key) = paired_ctx()?;
        let id = ctx
            .registry
            .add_light(EmulatedLight::new_color_candle("Edge", ""));
        let uuid = ctx
            .registry
            .light(&id)
            .ok_or("light present")?
            .id_v2
            .to_string();
        let after = |body: Value| {
            let _ = put_light(&ctx, Some(&key), &uuid, &body);
            ctx.registry.light(&id).ok_or("light present")
        };
        let dim = |pct: f64| {
            after(json!({ "dimming": { "brightness": pct } })).map(|l| l.brightness.to_bits())
        };
        let mirek = |n: i64| after(json!({ "color_temperature": { "mirek": n } })).map(|l| l.mirek);
        let xy = |point: Value| {
            after(json!({ "color": { "xy": point } }))
                .map(|l| l.xy.map(|(x, y)| (x.to_bits(), y.to_bits())))
        };

        // Brightness is narrowed from `f64` to the nearest `f32`, unclamped.
        assert_eq!(dim(55.5)?, 0x425e_0000);
        assert_eq!(dim(0.1)?, 0x3dcc_cccd);
        assert_eq!(dim(0.0)?, 0);
        assert_eq!(dim(-2.5)?, 0xc020_0000);
        assert_eq!(dim(1e40)?, 0x7f80_0000);
        assert_eq!(dim(-1e40)?, 0xff80_0000);
        assert_eq!(dim(1e-50)?, 0);

        // Mirek is clamped to 153..=500.
        assert_eq!(mirek(153)?, Some(153));
        assert_eq!(mirek(0)?, Some(153));
        assert_eq!(mirek(-1)?, Some(153));
        assert_eq!(mirek(i64::MIN)?, Some(153));
        assert_eq!(mirek(366)?, Some(366));
        assert_eq!(mirek(500)?, Some(500));
        assert_eq!(mirek(65_536 + 200)?, Some(500));
        assert_eq!(mirek(i64::MAX)?, Some(500));

        assert_eq!(
            xy(json!({ "x": 0.4573, "y": 0.41 }))?,
            Some((0x3eea_233a, 0x3ed1_eb85))
        );
        assert_eq!(
            xy(json!({ "x": 1e40, "y": -1e40 }))?,
            Some((0x7f80_0000, 0xff80_0000))
        );
        // A missing coordinate reads as 0.
        assert_eq!(xy(json!({ "x": 0.5 }))?, Some((0x3f00_0000, 0)));
        Ok(())
    }
}
