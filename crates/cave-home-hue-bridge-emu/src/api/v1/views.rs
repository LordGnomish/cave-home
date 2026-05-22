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
use crate::pairing::PairingService;
use crate::registry::{BridgeRegistry, EmulatedGroup, EmulatedLight, EmulatedScene, EmulatedSensor};
use serde_json::{Map, Value, json};

/// Short anonymous config — `GET /api/config`. Reference: §7.2.
#[must_use]
pub fn short_config(identity: &BridgeIdentity) -> Value {
    json!({
        "name": identity.name,
        "datastoreversion": identity.datastore_version,
        "swversion": identity.software_version,
        "apiversion": identity.api_version,
        "mac": identity.mac,
        "bridgeid": identity.bridge_id.to_uppercase(),
        "factorynew": false,
        "replacesbridgeid": Value::Null,
        "modelid": identity.model_id,
        "starterkitid": "",
    })
}

/// Full authenticated config. Reference: §7.2. Includes whitelist.
#[must_use]
pub fn full_config(identity: &BridgeIdentity, pairing: &PairingService) -> Value {
    let mut base = short_config(identity);
    let obj = base.as_object_mut().expect("short_config is an object");
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
    obj.insert("portalconnection".into(), Value::String("disconnected".into()));
    obj.insert("zigbeechannel".into(), Value::from(25));
    obj.insert("ipaddress".into(), Value::String(identity.host.clone()));
    obj.insert("netmask".into(), Value::String("255.255.255.0".into()));
    obj.insert("gateway".into(), Value::String("0.0.0.0".into()));
    obj.insert("dhcp".into(), Value::Bool(true));
    base
}

/// Render a single light to its v1 view. Reference: §1.4 `/lights/<id>` schema.
#[must_use]
pub fn light_view(light: &EmulatedLight) -> Value {
    let bri = (light.brightness / 100.0 * 254.0).round().clamp(1.0, 254.0) as u16;
    let xy: Vec<f64> = light
        .xy
        .map(|(x, y)| vec![x as f64, y as f64])
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

    #[test]
    fn short_config_uses_uppercase_bridge_id_per_docs() {
        let id = BridgeIdentity::fresh("10.0.0.1");
        let v = short_config(&id);
        assert_eq!(
            v.get("bridgeid").unwrap().as_str().unwrap(),
            id.bridge_id.to_uppercase()
        );
    }

    #[test]
    fn light_view_round_trip_bri_percent_to_v1_int() {
        let mut l = EmulatedLight::new_color_candle("X", "1");
        l.on = true;
        l.brightness = 50.0;
        let v = light_view(&l);
        let bri = v.get("state").unwrap().get("bri").unwrap().as_i64().unwrap();
        assert!(bri >= 126 && bri <= 128, "bri = {bri} expected ~127");
    }

    #[test]
    fn group_view_state_all_on_only_if_every_member_on() {
        let reg = BridgeRegistry::new();
        let l1 = reg.add_light(EmulatedLight::new_color_candle("A", ""));
        let l2 = reg.add_light(EmulatedLight::new_color_candle("B", ""));
        reg.update_light(&l1, |l| l.on = true);
        // l2 stays off
        let mut g = crate::registry::EmulatedGroup::new_room("R", "");
        g.member_lights_v1 = vec![l1.clone(), l2.clone()];
        let gid = reg.add_group(g);
        let v = group_view(&reg.group(&gid).unwrap(), &reg);
        let state = v.get("state").unwrap();
        assert_eq!(state.get("any_on").unwrap(), &Value::Bool(true));
        assert_eq!(state.get("all_on").unwrap(), &Value::Bool(false));
    }

    #[test]
    fn lights_map_keys_match_v1_ids() {
        let reg = BridgeRegistry::new();
        let id = reg.add_light(EmulatedLight::new_color_candle("L", ""));
        let map = lights_map(&reg);
        assert!(map.as_object().unwrap().contains_key(&id));
    }
}
