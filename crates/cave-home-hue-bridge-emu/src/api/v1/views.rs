// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! v1 JSON view rendering — turn registry records into wire-shape JSON.
//!
//! Reference: developer-portal Hue API v1 reference pages for each resource:
//! `/lights/<id>`, `/groups/<id>`, `/scenes/<id>`, `/sensors/<id>`, `/config`.
//! Field names/shapes match the published examples.

use crate::config::BridgeIdentity;
use crate::num::f32_to_u16;
use crate::pairing::PairingService;
use crate::registry::{
    BridgeRegistry, EmulatedGroup, EmulatedLight, EmulatedScene, EmulatedSensor,
};
use serde_json::{Map, Value, json};

/// The members of the short config, in published order. Shared by
/// [`short_config`] and [`full_config`], which extends them.
fn short_config_members(identity: &BridgeIdentity) -> Map<String, Value> {
    let mut obj = Map::new();
    obj.insert("name".into(), json!(identity.name));
    obj.insert("datastoreversion".into(), json!(identity.datastore_version));
    obj.insert("swversion".into(), json!(identity.software_version));
    obj.insert("apiversion".into(), json!(identity.api_version));
    obj.insert("mac".into(), json!(identity.mac));
    obj.insert("bridgeid".into(), json!(identity.bridge_id.to_uppercase()));
    obj.insert("factorynew".into(), json!(false));
    obj.insert("replacesbridgeid".into(), Value::Null);
    obj.insert("modelid".into(), json!(identity.model_id));
    obj.insert("starterkitid".into(), json!(""));
    obj
}

/// Short anonymous config — `GET /api/config`. Reference: §7.2.
#[must_use]
pub fn short_config(identity: &BridgeIdentity) -> Value {
    Value::Object(short_config_members(identity))
}

/// Full authenticated config. Reference: §7.2. Includes whitelist.
#[must_use]
pub fn full_config(identity: &BridgeIdentity, pairing: &PairingService) -> Value {
    let mut obj = short_config_members(identity);
    // Whitelist map keyed by app_key — published shape.
    let mut whitelist = Map::new();
    for entry in pairing.whitelist_all() {
        whitelist.insert(
            entry.app_key.clone(),
            json!({
                "last use date": entry.create_date,
                "create date": entry.create_date,
                "name": entry.device_type,
            }),
        );
    }
    obj.insert("whitelist".into(), Value::Object(whitelist));
    obj.insert(
        "linkbutton".into(),
        Value::Bool(pairing.is_link_button_pressed()),
    );
    obj.insert("portalservices".into(), Value::Bool(false));
    obj.insert(
        "portalconnection".into(),
        Value::String("disconnected".into()),
    );
    obj.insert("zigbeechannel".into(), Value::from(25));
    obj.insert("ipaddress".into(), Value::String(identity.host.clone()));
    obj.insert("netmask".into(), Value::String("255.255.255.0".into()));
    obj.insert("gateway".into(), Value::String("0.0.0.0".into()));
    obj.insert("dhcp".into(), Value::Bool(true));
    Value::Object(obj)
}

/// Render a single light to its v1 view. Reference: §1.4 `/lights/<id>` schema.
#[must_use]
pub fn light_view(light: &EmulatedLight) -> Value {
    let bri = f32_to_u16((light.brightness / 100.0 * 254.0).round().clamp(1.0, 254.0));
    let xy: Vec<f64> = light
        .xy
        .map(|(x, y)| vec![f64::from(x), f64::from(y)])
        .unwrap_or_default();
    let ct = light.mirek.unwrap_or(366);
    let colormode = if light.xy.is_some() {
        "xy"
    } else if light.mirek.is_some() {
        "ct"
    } else {
        "hs"
    };
    json!({
        "state": {
            "on": light.on,
            "bri": bri,
            "hue": 8418,
            "sat": 140,
            "effect": "none",
            "xy": xy,
            "ct": ct,
            "alert": "none",
            "colormode": colormode,
            "mode": "homeautomation",
            "reachable": true
        },
        "swupdate": {"state": "noupdates", "lastinstall": null},
        "type": light.light_type,
        "name": light.name,
        "modelid": light.model_id,
        "manufacturername": light.manufacturer_name,
        "productname": "Hue color candle",
        "capabilities": {
            "certified": true,
            "control": {
                "mindimlevel": 2000,
                "maxlumen": 800,
                "colorgamuttype": "C",
                "colorgamut": [
                    [0.6915, 0.3083],
                    [0.17, 0.7],
                    [0.1532, 0.0475]
                ],
                "ct": {"min": 153, "max": 500}
            },
            "streaming": {"renderer": true, "proxy": true}
        },
        "config": {
            "archetype": "candlebulb",
            "function": "mixed",
            "direction": "omnidirectional"
        },
        "uniqueid": format!("00:17:88:01:00:00:{:02x}:{:02x}-0b",
            light.id_v2.as_bytes()[14], light.id_v2.as_bytes()[15]),
        "swversion": "1.108.10"
    })
}

/// All lights, keyed by v1 id.
#[must_use]
pub fn lights_map(registry: &BridgeRegistry) -> Value {
    let mut map = Map::new();
    for light in registry.lights() {
        map.insert(light.id_v1.clone(), light_view(&light));
    }
    Value::Object(map)
}

/// Render a single group. Reference: §4.4 `/groups/<id>`.
#[must_use]
pub fn group_view(group: &EmulatedGroup, registry: &BridgeRegistry) -> Value {
    let lights: Vec<&str> = group.member_lights_v1.iter().map(String::as_str).collect();
    let any_on = group
        .member_lights_v1
        .iter()
        .filter_map(|id| registry.light(id))
        .any(|l| l.on);
    let all_on = !group.member_lights_v1.is_empty()
        && group
            .member_lights_v1
            .iter()
            .filter_map(|id| registry.light(id))
            .all(|l| l.on);
    json!({
        "name": group.name,
        "lights": lights,
        "sensors": Vec::<&str>::new(),
        "type": group.group_type,
        "state": {"all_on": all_on, "any_on": any_on},
        "recycle": false,
        "class": "Living room",
        "action": {
            "on": any_on,
            "bri": 254,
            "hue": 0,
            "sat": 0,
            "effect": "none",
            "xy": [0.0, 0.0],
            "ct": 366,
            "alert": "none",
            "colormode": "xy"
        }
    })
}

/// All groups, keyed by v1 id.
#[must_use]
pub fn groups_map(registry: &BridgeRegistry) -> Value {
    let mut map = Map::new();
    for group in registry.groups() {
        map.insert(group.id_v1.clone(), group_view(&group, registry));
    }
    Value::Object(map)
}

/// Render a single scene. Reference: §5.4 `/scenes/<id>`.
#[must_use]
pub fn scene_view(scene: &EmulatedScene) -> Value {
    let lights: Vec<String> = scene.actions.keys().cloned().collect();
    json!({
        "name": scene.name,
        "lights": lights,
        "owner": "cave-home-emu",
        "recycle": false,
        "locked": false,
        "appdata": {"version": 1, "data": "cave"},
        "picture": "",
        "lastupdated": "2026-05-17T20:00:00",
        "version": 2,
        "type": "GroupScene",
        "group": scene.group_v1,
    })
}

/// All scenes, keyed by v1 id.
#[must_use]
pub fn scenes_map(registry: &BridgeRegistry) -> Value {
    let mut map = Map::new();
    for scene in registry.scenes() {
        map.insert(scene.id_v1.clone(), scene_view(&scene));
    }
    Value::Object(map)
}

/// Render a single sensor. Reference: §2.4 `/sensors/<id>`.
#[must_use]
pub fn sensor_view(sensor: &EmulatedSensor) -> Value {
    json!({
        "state": sensor.state,
        "config": sensor.config,
        "name": sensor.name,
        "type": sensor.sensor_type,
        "modelid": "RWL022",
        "manufacturername": "Signify Netherlands B.V.",
        "swversion": "100.0.0",
        "uniqueid": format!("00:17:88:01:00:00:{:02x}:{:02x}-02-fc00",
            sensor.id_v2.as_bytes()[14], sensor.id_v2.as_bytes()[15]),
    })
}

/// All sensors, keyed by v1 id.
#[must_use]
pub fn sensors_map(registry: &BridgeRegistry) -> Value {
    let mut map = Map::new();
    for sensor in registry.sensors() {
        map.insert(sensor.id_v1.clone(), sensor_view(&sensor));
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::EmulatedLight;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn short_config_uses_uppercase_bridge_id_per_docs() -> TestResult {
        let id = BridgeIdentity::fresh("10.0.0.1");
        let v = short_config(&id);
        assert_eq!(
            v.get("bridgeid")
                .ok_or("expected Some, got None")?
                .as_str()
                .ok_or("expected Some, got None")?,
            id.bridge_id.to_uppercase()
        );
        Ok(())
    }

    #[test]
    fn light_view_round_trip_bri_percent_to_v1_int() -> TestResult {
        let mut l = EmulatedLight::new_color_candle("X", "1");
        l.on = true;
        l.brightness = 50.0;
        let v = light_view(&l);
        let bri = v
            .get("state")
            .ok_or("expected Some, got None")?
            .get("bri")
            .ok_or("expected Some, got None")?
            .as_i64()
            .ok_or("expected Some, got None")?;
        assert!((126..=128).contains(&bri), "bri = {bri} expected ~127");
        Ok(())
    }

    #[test]
    fn group_view_state_all_on_only_if_every_member_on() -> TestResult {
        let reg = BridgeRegistry::new();
        let l1 = reg.add_light(EmulatedLight::new_color_candle("A", ""));
        let l2 = reg.add_light(EmulatedLight::new_color_candle("B", ""));
        reg.update_light(&l1, |l| l.on = true);
        // l2 stays off
        let mut g = crate::registry::EmulatedGroup::new_room("R", "");
        g.member_lights_v1 = vec![l1, l2];
        let gid = reg.add_group(g);
        let v = group_view(&reg.group(&gid).ok_or("expected Some, got None")?, &reg);
        let state = v.get("state").ok_or("expected Some, got None")?;
        assert_eq!(
            state.get("any_on").ok_or("expected Some, got None")?,
            &Value::Bool(true)
        );
        assert_eq!(
            state.get("all_on").ok_or("expected Some, got None")?,
            &Value::Bool(false)
        );
        Ok(())
    }

    #[test]
    fn lights_map_keys_match_v1_ids() -> TestResult {
        let reg = BridgeRegistry::new();
        let id = reg.add_light(EmulatedLight::new_color_candle("L", ""));
        let map = lights_map(&reg);
        assert!(
            map.as_object()
                .ok_or("expected Some, got None")?
                .contains_key(&id)
        );
        Ok(())
    }

    #[test]
    fn light_view_bri_edge_values() -> TestResult {
        let bri_for = |brightness: f32| {
            let mut l = EmulatedLight::new_color_candle("X", "1");
            l.brightness = brightness;
            light_view(&l)
                .get("state")
                .and_then(|s| s.get("bri"))
                .and_then(Value::as_u64)
                .ok_or("bri present")
        };
        // The percentage is rounded to the 1..=254 v1 scale and clamped.
        assert_eq!(bri_for(0.0)?, 1);
        assert_eq!(bri_for(-5.0)?, 1);
        assert_eq!(bri_for(0.2)?, 1);
        assert_eq!(bri_for(0.59)?, 1);
        assert_eq!(bri_for(0.6)?, 2);
        assert_eq!(bri_for(50.0)?, 127);
        assert_eq!(bri_for(99.9)?, 254);
        assert_eq!(bri_for(100.0)?, 254);
        assert_eq!(bri_for(250.0)?, 254);
        assert_eq!(bri_for(f32::INFINITY)?, 254);
        assert_eq!(bri_for(f32::NEG_INFINITY)?, 1);
        // A NaN brightness passes through the clamp and reads as 0.
        assert_eq!(bri_for(f32::NAN)?, 0);
        Ok(())
    }

    #[test]
    fn light_view_xy_is_widened_exactly() -> TestResult {
        let mut l = EmulatedLight::new_color_candle("X", "1");
        l.xy = Some((0.4, 0.25));
        let v = light_view(&l);
        let xy = v
            .get("state")
            .and_then(|s| s.get("xy"))
            .and_then(Value::as_array)
            .ok_or("xy present")?;
        let bits: Vec<Option<u64>> = xy.iter().map(|n| n.as_f64().map(f64::to_bits)).collect();
        // 0.4_f32 is 0x3ecc_cccd; widening keeps exactly those mantissa bits.
        assert_eq!(
            bits,
            [Some(0x3fd9_9999_a000_0000), Some(0x3fd0_0000_0000_0000)]
        );
        Ok(())
    }

    #[test]
    fn short_config_has_exactly_the_documented_keys() {
        let id = BridgeIdentity::fresh("10.0.0.1");
        let expected = json!({
            "name": id.name,
            "datastoreversion": id.datastore_version,
            "swversion": id.software_version,
            "apiversion": id.api_version,
            "mac": id.mac,
            "bridgeid": id.bridge_id.to_uppercase(),
            "factorynew": false,
            "replacesbridgeid": Value::Null,
            "modelid": id.model_id,
            "starterkitid": "",
        });
        assert_eq!(short_config(&id), expected);
        assert_eq!(
            serde_json::to_string(&short_config(&id)).ok(),
            serde_json::to_string(&expected).ok()
        );
    }

    #[test]
    fn full_config_extends_the_short_config() -> TestResult {
        let id = BridgeIdentity::fresh("10.0.0.1");
        let pairing = PairingService::new();
        let short = short_config(&id);
        let full = full_config(&id, &pairing);
        let short = short.as_object().ok_or("short config is an object")?;
        let full_obj = full.as_object().ok_or("full config is an object")?;
        for (key, value) in short {
            assert_eq!(full_obj.get(key), Some(value), "key {key}");
        }
        let extra: Vec<&str> = full_obj
            .keys()
            .map(String::as_str)
            .filter(|k| !short.contains_key(*k))
            .collect();
        let mut sorted = extra.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            [
                "dhcp",
                "gateway",
                "ipaddress",
                "linkbutton",
                "netmask",
                "portalconnection",
                "portalservices",
                "whitelist",
                "zigbeechannel",
            ]
        );
        assert_eq!(full_obj.len(), short.len() + 9);

        // Byte-for-byte: extending the rendered short config in place gives
        // the same serialised object, key order included.
        let mut expected = short_config(&id);
        let obj = expected
            .as_object_mut()
            .ok_or("short config is an object")?;
        obj.insert("whitelist".into(), json!({}));
        obj.insert("linkbutton".into(), json!(false));
        obj.insert("portalservices".into(), json!(false));
        obj.insert("portalconnection".into(), json!("disconnected"));
        obj.insert("zigbeechannel".into(), json!(25));
        obj.insert("ipaddress".into(), json!("10.0.0.1"));
        obj.insert("netmask".into(), json!("255.255.255.0"));
        obj.insert("gateway".into(), json!("0.0.0.0"));
        obj.insert("dhcp".into(), json!(true));
        assert_eq!(full, expected);
        assert_eq!(
            serde_json::to_string(&full)?,
            serde_json::to_string(&expected)?
        );
        Ok(())
    }
}
