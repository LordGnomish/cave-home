// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! v1 REST endpoints — `/api/<appkey>/...` family.
//!
//! Endpoints covered:
//!  - `POST /api` — pairing (delegates to [`crate::pairing`]).
//!  - `GET /api/<appkey>` — full state.
//!  - `GET /api/<appkey>/config` — bridge config.
//!  - `GET /api/config` — anonymous short config (used by discovery probes).
//!  - `GET /api/<appkey>/lights` — lights map.
//!  - `GET /api/<appkey>/lights/<id>` — single light.
//!  - `PUT /api/<appkey>/lights/<id>/state` — set light state.
//!  - `GET /api/<appkey>/groups` — groups map.
//!  - `PUT /api/<appkey>/groups/<id>/action` — group action / scene recall.
//!  - `GET /api/<appkey>/scenes` — scenes map.
//!  - `GET /api/<appkey>/sensors` — sensors map.
//!
//! Reference: developer-portal "Hue API v1" full reference.

pub mod views;

use crate::config::BridgeIdentity;
use crate::errors::{HueProtocolError, emu_to_protocol};
use crate::pairing::{PairRequest, PairingService};
use crate::registry::BridgeRegistry;
use serde_json::{Map, Value, json};
use std::sync::Arc;

/// Result wrapper for an API call. Either a JSON value (the bridge always
/// wraps "ok" responses in a top-level shape that depends on the endpoint)
/// or a Hue protocol error array.
pub type ApiResult = Result<Value, Vec<HueProtocolError>>;

/// State the v1 dispatcher needs.
#[derive(Clone)]
pub struct V1Context {
    pub identity: BridgeIdentity,
    pub pairing: PairingService,
    pub registry: Arc<BridgeRegistry>,
}

/// Handle `POST /api`. Returns the standard `[{"success":{"username": ...}}]`
/// or `[{"error":{...}}]` payload.
pub fn pair(ctx: &V1Context, body: &Value) -> Value {
    let req: PairRequest = match serde_json::from_value(body.clone()) {
        Ok(v) => v,
        Err(err) => {
            return json!([{ "error": err_obj(&HueProtocolError {
                kind: crate::errors::error_type::BODY_CONTAINS_INVALID_JSON,
                address: "/api".into(),
                description: format!("body contains invalid json: {err}"),
            }) }]);
        }
    };
    match ctx.pairing.try_pair(&req, "2026-05-17T20:00:00") {
        Ok(success) => {
            let mut obj = Map::new();
            obj.insert("username".into(), Value::String(success.username));
            if let Some(ck) = success.clientkey {
                obj.insert("clientkey".into(), Value::String(ck));
            }
            json!([{ "success": Value::Object(obj) }])
        }
        Err(err) => {
            let p = emu_to_protocol(&err, "/api");
            json!([{ "error": err_obj(&p) }])
        }
    }
}

/// Validate that `app_key` is whitelisted. Otherwise return an unauthorized
/// envelope at `/<address>`.
pub fn require_app_key(ctx: &V1Context, app_key: &str, address: &str) -> Result<(), Value> {
    if ctx.pairing.whitelist_get(app_key).is_some() {
        Ok(())
    } else {
        let p = HueProtocolError::unauthorized(address);
        Err(json!([{ "error": err_obj(&p) }]))
    }
}

/// `GET /api/config` — anonymous short config. Reference: dev-portal §7.2.
pub fn anonymous_config(ctx: &V1Context) -> Value {
    views::short_config(&ctx.identity)
}

/// `GET /api/<appkey>/config` — authenticated config.
pub fn full_config(ctx: &V1Context, app_key: &str) -> ApiResult {
    if let Err(_) = require_app_key(ctx, app_key, &format!("/api/{app_key}/config")) {
        return Err(vec![HueProtocolError::unauthorized(format!(
            "/api/{app_key}/config"
        ))]);
    }
    Ok(views::full_config(&ctx.identity, &ctx.pairing))
}

/// `GET /api/<appkey>` — entire datastore.
pub fn full_state(ctx: &V1Context, app_key: &str) -> ApiResult {
    if let Err(_) = require_app_key(ctx, app_key, &format!("/api/{app_key}")) {
        return Err(vec![HueProtocolError::unauthorized(format!("/api/{app_key}"))]);
    }
    let lights = views::lights_map(&ctx.registry);
    let groups = views::groups_map(&ctx.registry);
    let scenes = views::scenes_map(&ctx.registry);
    let sensors = views::sensors_map(&ctx.registry);
    Ok(json!({
        "lights": lights,
        "groups": groups,
        "scenes": scenes,
        "sensors": sensors,
        "config": views::full_config(&ctx.identity, &ctx.pairing),
    }))
}

/// `GET /api/<appkey>/lights`.
pub fn get_lights(ctx: &V1Context, app_key: &str) -> ApiResult {
    if let Err(_) = require_app_key(ctx, app_key, &format!("/api/{app_key}/lights")) {
        return Err(vec![HueProtocolError::unauthorized(format!(
            "/api/{app_key}/lights"
        ))]);
    }
    Ok(views::lights_map(&ctx.registry))
}

/// `GET /api/<appkey>/lights/<id>`.
pub fn get_light(ctx: &V1Context, app_key: &str, id: &str) -> ApiResult {
    let address = format!("/api/{app_key}/lights/{id}");
    if let Err(_) = require_app_key(ctx, app_key, &address) {
        return Err(vec![HueProtocolError::unauthorized(address)]);
    }
    match ctx.registry.light(id) {
        Some(light) => Ok(views::light_view(&light)),
        None => Err(vec![HueProtocolError::not_found(format!("/lights/{id}"))]),
    }
}

/// `PUT /api/<appkey>/lights/<id>/state` — apply a state patch.
///
/// Body schema (per dev-portal §1.6): `{on, bri, hue, sat, xy, ct, alert,
/// effect, transitiontime, *_inc}`. We support `on`, `bri`, `xy`, `ct`.
pub fn put_light_state(
    ctx: &V1Context,
    app_key: &str,
    id: &str,
    body: &Value,
) -> Value {
    let address = format!("/api/{app_key}/lights/{id}/state");
    if let Err(envelope) = require_app_key(ctx, app_key, &address) {
        return envelope;
    }
    let Some(_existing) = ctx.registry.light(id) else {
        let p = HueProtocolError::not_found(format!("/lights/{id}"));
        return json!([{ "error": err_obj(&p) }]);
    };
    let mut acks: Vec<Value> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();
    let Some(obj) = body.as_object() else {
        let p = HueProtocolError {
            kind: crate::errors::error_type::BODY_CONTAINS_INVALID_JSON,
            address: address.clone(),
            description: "body is not an object".into(),
        };
        return json!([{ "error": err_obj(&p) }]);
    };

    let snapshot = ctx.registry.update_light(id, |light| {
        if let Some(v) = obj.get("on").and_then(Value::as_bool) {
            light.on = v;
            acks.push(json!({ "success": { format!("/lights/{id}/state/on"): v } }));
        }
        if let Some(v) = obj.get("bri").and_then(Value::as_i64) {
            let clamped = v.clamp(1, 254);
            light.brightness = clamped as f32 / 254.0 * 100.0;
            acks.push(json!({ "success": { format!("/lights/{id}/state/bri"): clamped } }));
        }
        if let Some(arr) = obj.get("xy").and_then(Value::as_array) {
            if arr.len() == 2 {
                let x = arr[0].as_f64().unwrap_or(0.0) as f32;
                let y = arr[1].as_f64().unwrap_or(0.0) as f32;
                light.xy = Some((x, y));
                acks.push(json!({ "success": { format!("/lights/{id}/state/xy"): [x, y] } }));
            }
        }
        if let Some(v) = obj.get("ct").and_then(Value::as_i64) {
            let clamped = v.clamp(153, 500) as u16;
            light.mirek = Some(clamped);
            acks.push(json!({ "success": { format!("/lights/{id}/state/ct"): clamped } }));
        }
    });

    if snapshot.is_none() {
        let p = HueProtocolError::internal("light disappeared during update");
        errors.push(json!({ "error": err_obj(&p) }));
    }

    let mut out = acks;
    out.extend(errors);
    Value::Array(out)
}

/// `GET /api/<appkey>/groups`.
pub fn get_groups(ctx: &V1Context, app_key: &str) -> ApiResult {
    let address = format!("/api/{app_key}/groups");
    if let Err(_) = require_app_key(ctx, app_key, &address) {
        return Err(vec![HueProtocolError::unauthorized(address)]);
    }
    Ok(views::groups_map(&ctx.registry))
}

/// `PUT /api/<appkey>/groups/<id>/action`. Body supports `on`, `bri`, `xy`,
/// `ct`, `scene`.
pub fn put_group_action(ctx: &V1Context, app_key: &str, id: &str, body: &Value) -> Value {
    let address = format!("/api/{app_key}/groups/{id}/action");
    if let Err(envelope) = require_app_key(ctx, app_key, &address) {
        return envelope;
    }
    let Some(obj) = body.as_object() else {
        let p = HueProtocolError {
            kind: crate::errors::error_type::BODY_CONTAINS_INVALID_JSON,
            address,
            description: "body is not an object".into(),
        };
        return json!([{ "error": err_obj(&p) }]);
    };

    // Special-case scene recall — applies the scene's stored actions.
    if let Some(scene_id) = obj.get("scene").and_then(Value::as_str) {
        let recalled = ctx.registry.recall_scene(scene_id);
        return json!([{
            "success": { format!("/groups/{id}/action/scene"): scene_id, "recalled": recalled }
        }]);
    }

    // Otherwise fan out to member lights (group 0 = all lights).
    let lights = if id == "0" {
        ctx.registry.lights().into_iter().map(|l| l.id_v1).collect()
    } else if let Some(g) = ctx.registry.group(id) {
        g.member_lights_v1
    } else {
        let p = HueProtocolError::not_found(format!("/groups/{id}"));
        return json!([{ "error": err_obj(&p) }]);
    };

    let mut acks = Vec::new();
    for light_id in &lights {
        ctx.registry.update_light(light_id, |light| {
            if let Some(v) = obj.get("on").and_then(Value::as_bool) {
                light.on = v;
            }
            if let Some(v) = obj.get("bri").and_then(Value::as_i64) {
                let clamped = v.clamp(1, 254);
                light.brightness = clamped as f32 / 254.0 * 100.0;
            }
            if let Some(arr) = obj.get("xy").and_then(Value::as_array) {
                if arr.len() == 2 {
                    light.xy = Some((
                        arr[0].as_f64().unwrap_or(0.0) as f32,
                        arr[1].as_f64().unwrap_or(0.0) as f32,
                    ));
                }
            }
            if let Some(v) = obj.get("ct").and_then(Value::as_i64) {
                light.mirek = Some(v.clamp(153, 500) as u16);
            }
        });
    }
    for (key, value) in obj {
        acks.push(json!({
            "success": { format!("/groups/{id}/action/{key}"): value }
        }));
    }
    Value::Array(acks)
}

/// `GET /api/<appkey>/scenes`.
pub fn get_scenes(ctx: &V1Context, app_key: &str) -> ApiResult {
    let address = format!("/api/{app_key}/scenes");
    if let Err(_) = require_app_key(ctx, app_key, &address) {
        return Err(vec![HueProtocolError::unauthorized(address)]);
    }
    Ok(views::scenes_map(&ctx.registry))
}

/// `GET /api/<appkey>/sensors`.
pub fn get_sensors(ctx: &V1Context, app_key: &str) -> ApiResult {
    let address = format!("/api/{app_key}/sensors");
    if let Err(_) = require_app_key(ctx, app_key, &address) {
        return Err(vec![HueProtocolError::unauthorized(address)]);
    }
    Ok(views::sensors_map(&ctx.registry))
}

/// Helper to serialise a `HueProtocolError` as a plain `Value`.
fn err_obj(p: &HueProtocolError) -> Value {
    serde_json::to_value(p).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::EmulatedLight;

    fn fresh_ctx() -> V1Context {
        let identity = BridgeIdentity::fresh("10.0.0.5");
        let pairing = PairingService::new();
        let registry = BridgeRegistry::new();
        V1Context {
            identity,
            pairing,
            registry,
        }
    }

    fn paired_ctx() -> (V1Context, String) {
        let ctx = fresh_ctx();
        ctx.pairing.begin_link_window();
        let key = ctx
            .pairing
            .try_pair(
                &PairRequest {
                    devicetype: "cave-home#test".into(),
                    generateclientkey: false,
                },
                "t",
            )
            .unwrap()
            .username;
        (ctx, key)
    }

    #[test]
    fn pair_without_button_returns_link_button_error() {
        let ctx = fresh_ctx();
        let resp = pair(&ctx, &json!({"devicetype": "x#y"}));
        let arr = resp.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let err = arr[0].get("error").unwrap();
        assert_eq!(err.get("type").unwrap(), &json!(101));
    }

    #[test]
    fn pair_after_button_press_returns_username() {
        let ctx = fresh_ctx();
        ctx.pairing.begin_link_window();
        let resp = pair(&ctx, &json!({"devicetype": "x#y"}));
        let arr = resp.as_array().unwrap();
        let success = arr[0].get("success").unwrap();
        assert!(success.get("username").unwrap().is_string());
        assert!(success.get("clientkey").is_none());
    }

    #[test]
    fn pair_with_generateclientkey_returns_clientkey() {
        let ctx = fresh_ctx();
        ctx.pairing.begin_link_window();
        let resp = pair(
            &ctx,
            &json!({"devicetype": "x#y", "generateclientkey": true}),
        );
        let arr = resp.as_array().unwrap();
        let success = arr[0].get("success").unwrap();
        assert!(success.get("clientkey").unwrap().is_string());
    }

    #[test]
    fn anonymous_config_includes_bridgeid_and_modelid() {
        let ctx = fresh_ctx();
        let cfg = anonymous_config(&ctx);
        assert!(cfg.get("bridgeid").is_some());
        assert_eq!(cfg.get("modelid").unwrap(), &json!("BSB002"));
        assert!(cfg.get("apiversion").is_some());
    }

    #[test]
    fn get_lights_unauthorized_without_app_key() {
        let ctx = fresh_ctx();
        let result = get_lights(&ctx, "wrong-key");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err()[0].kind, 1);
    }

    #[test]
    fn round_trip_pair_then_create_light_then_set_state() {
        let (ctx, key) = paired_ctx();
        let id = ctx
            .registry
            .add_light(EmulatedLight::new_color_candle("Mutfak", ""));
        // Set on=true, bri=200.
        let resp = put_light_state(
            &ctx,
            &key,
            &id,
            &json!({"on": true, "bri": 200}),
        );
        let arr = resp.as_array().unwrap();
        assert!(arr.iter().any(|v| v.get("success").is_some()));
        let snap = ctx.registry.light(&id).unwrap();
        assert!(snap.on);
        assert!(snap.brightness > 78.0 && snap.brightness < 79.0); // 200/254*100 = 78.74...
    }

    #[test]
    fn put_unknown_light_returns_not_found() {
        let (ctx, key) = paired_ctx();
        let resp = put_light_state(&ctx, &key, "99", &json!({"on": true}));
        let arr = resp.as_array().unwrap();
        let err = arr[0].get("error").unwrap();
        assert_eq!(err.get("type").unwrap(), &json!(3));
    }

    #[test]
    fn full_state_for_known_app_key_returns_all_sections() {
        let (ctx, key) = paired_ctx();
        let _ = ctx
            .registry
            .add_light(EmulatedLight::new_color_candle("L1", ""));
        let value = full_state(&ctx, &key).unwrap();
        for s in ["lights", "groups", "scenes", "sensors", "config"] {
            assert!(value.get(s).is_some(), "missing section {s}");
        }
        assert_eq!(value.get("lights").unwrap().as_object().unwrap().len(), 1);
    }
}
