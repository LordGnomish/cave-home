// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! Integration test: simulate a third-party Hue app talking to the
//! emulator end-to-end.
//!
//! Covers the three slices the mandate calls out:
//!   1. Fake Hue app pairing flow (`POST /api` + link button window).
//!   2. Light state round-trip (`PUT` then `GET`).
//!   3. EventStream subscription receives the resulting `update` event.

use cave_home_hue_bridge_emu::api::v1::{V1Context, get_light, pair, put_light_state};
use cave_home_hue_bridge_emu::api::v2::eventstream::render_event;
use cave_home_hue_bridge_emu::config::BridgeIdentity;
use cave_home_hue_bridge_emu::pairing::PairingService;
use cave_home_hue_bridge_emu::registry::{BridgeRegistry, EmulatedLight, StreamEventKind};
use serde_json::json;

fn fresh_ctx() -> V1Context {
    V1Context {
        identity: BridgeIdentity::fresh("10.0.0.42"),
        pairing: PairingService::new(),
        registry: BridgeRegistry::new(),
    }
}

#[tokio::test]
async fn fake_hue_app_pairs_then_toggles_light_and_observes_eventstream() {
    let ctx = fresh_ctx();

    // ----- Step 1: fake Hue app posts to /api before the button is pressed.
    let pre = pair(&ctx, &json!({"devicetype": "fake-app#test"}));
    let pre_arr = pre.as_array().unwrap();
    let pre_err = pre_arr[0].get("error").unwrap();
    assert_eq!(
        pre_err.get("type").unwrap(),
        &json!(101),
        "expect link-button-not-pressed before button press"
    );

    // ----- Step 2: cave-home admin clicks the (virtual) link button.
    ctx.pairing.begin_link_window();

    // ----- Step 3: app retries POST and gets back a username.
    let post = pair(
        &ctx,
        &json!({"devicetype": "fake-app#test", "generateclientkey": true}),
    );
    let post_arr = post.as_array().unwrap();
    let success = post_arr[0].get("success").unwrap();
    let app_key = success.get("username").unwrap().as_str().unwrap().to_string();
    let clientkey = success.get("clientkey").unwrap().as_str().unwrap();
    assert_eq!(app_key.len(), 40, "username = 40 hex chars per docs");
    assert_eq!(clientkey.len(), 32, "clientkey = 32 hex chars per docs");

    // ----- Step 4: admin pre-provisions a light.
    let light_id = ctx
        .registry
        .add_light(EmulatedLight::new_color_candle("Mutfak", ""));

    // ----- Step 5: subscribe to the eventstream *before* mutating.
    let mut sub = ctx.registry.subscribe();

    // ----- Step 6: fake app turns the light on.
    let put_resp = put_light_state(
        &ctx,
        &app_key,
        &light_id,
        &json!({"on": true, "bri": 254}),
    );
    let acks = put_resp.as_array().unwrap();
    assert!(acks.iter().any(|v| v.get("success").is_some()));

    // ----- Step 7: GET the light back, confirm new state.
    let get_resp = get_light(&ctx, &app_key, &light_id).unwrap();
    let state = get_resp.get("state").unwrap();
    assert_eq!(state.get("on").unwrap(), &json!(true));
    let bri = state.get("bri").unwrap().as_i64().unwrap();
    assert!(bri >= 253);

    // ----- Step 8: EventStream subscriber received the update event.
    let event = tokio::time::timeout(std::time::Duration::from_millis(200), sub.recv())
        .await
        .expect("eventstream must deliver within 200ms")
        .expect("event Ok");
    assert_eq!(event.kind, StreamEventKind::Update);
    assert_eq!(event.data[0].get("type").unwrap(), "light");

    // ----- Step 9: SSE renderer produces a wire-format message.
    let rendered = render_event(&event);
    assert!(rendered.starts_with("id: "));
    assert!(rendered.contains("event: update\n"));
    assert!(rendered.contains("data: ["));
}

#[tokio::test]
async fn unauthorised_app_key_is_rejected_for_v1_and_v2() {
    let ctx = fresh_ctx();
    let light_id = ctx
        .registry
        .add_light(EmulatedLight::new_color_candle("X", ""));

    // v1: get_light with random key
    let v1_resp = get_light(&ctx, "00000000-not-paired-00000000", &light_id);
    let err = v1_resp.unwrap_err();
    assert_eq!(err[0].kind, 1, "v1 unauthorized = type 1");

    // v2: get_lights with no key
    let env = cave_home_hue_bridge_emu::api::v2::get_lights(&ctx, None);
    assert_eq!(env.errors.len(), 1);
    assert!(env.data.is_empty());
}

#[tokio::test]
async fn v2_full_resource_dump_includes_documented_types() {
    let ctx = fresh_ctx();
    ctx.pairing.begin_link_window();
    let key = pair(&ctx, &json!({"devicetype": "fake#test"}))
        .as_array()
        .unwrap()[0]
        .get("success")
        .unwrap()
        .get("username")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let _ = ctx
        .registry
        .add_light(EmulatedLight::new_color_candle("L", ""));
    let _ = ctx.registry.add_group(
        cave_home_hue_bridge_emu::registry::EmulatedGroup::new_room("Salon", ""),
    );
    let env = cave_home_hue_bridge_emu::api::v2::get_all_resources(&ctx, Some(&key));
    // Every resource carries a `type` field — verify required types are in there.
    let types: std::collections::HashSet<&str> = env
        .data
        .iter()
        .filter_map(|v| v.get("type").and_then(|s| s.as_str()))
        .collect();
    for t in ["bridge", "light", "device", "room", "grouped_light"] {
        assert!(types.contains(t), "missing v2 resource type: {t}");
    }
}
