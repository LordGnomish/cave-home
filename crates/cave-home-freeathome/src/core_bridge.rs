// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Bridge from free@home devices into the cave-home-core entity registry.
//!
//! Each free@home channel becomes one cave-home entity: the domain is the
//! grandma-friendly [`DeviceKind`](cave_home_free_home::DeviceKind) tag and the
//! object id is a stable, slug-safe `freeathome_<serial>_<channel>`. Once
//! registered, automations, the portal and voice treat a free@home light
//! exactly like any other light.

use cave_home_core::{Context, EntityId, StateAttributes, StateChange, StateMachine};
use serde_json::Value as JsonValue;

use crate::device::FreeAtHomeDevice;
use crate::error::{FreeAtHomeError, Result};

/// Lowercase a serial into the slug grammar core requires (`[a-z0-9_]`).
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// The cave-home [`EntityId`] for a free@home device.
pub fn entity_id(device: &dyn FreeAtHomeDevice) -> Result<EntityId> {
    let object_id = format!(
        "freeathome_{}_{}",
        slug(device.serial().as_str()),
        device.channel().index()
    );
    EntityId::new(device.kind().tag(), object_id)
        .map_err(|e| FreeAtHomeError::Domain(e.to_string()))
}

/// Map an on/off wire value to a Home-Assistant-style state token.
pub fn on_off_state(wire: Option<&str>) -> &'static str {
    match wire {
        Some("1") => "on",
        Some("0") => "off",
        _ => "unknown",
    }
}

/// Register (or refresh) a device's state in the core state machine.
///
/// `primary_value` is the device's main on/off datapoint wire value, if known.
/// Returns the [`StateChange`] (or `None` when nothing changed).
pub fn register(
    sm: &StateMachine,
    device: &dyn FreeAtHomeDevice,
    primary_value: Option<&str>,
) -> Result<Option<StateChange>> {
    let id = entity_id(device)?;
    let mut attributes = StateAttributes::new();
    attributes.insert(
        "friendly_name".into(),
        JsonValue::String(device.friendly_name().to_string()),
    );
    attributes.insert(
        "free_at_home_function".into(),
        JsonValue::String(format!("{:?}", device.function())),
    );
    if let Some(room) = device.room() {
        attributes.insert("room".into(), JsonValue::String(room.to_string()));
    }
    Ok(sm.set(id, on_off_state(primary_value), attributes, Context::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::Device;
    use cave_home_core::{EventBus, StateMachine};
    use cave_home_free_home::{Channel, ChannelId, DeviceSerial, Function};

    fn light() -> Device {
        Device::new(
            DeviceSerial::parse("ABB700C12345").expect("serial"),
            Channel::new(
                ChannelId::new(0),
                Function::DimmingActuator,
                Some("Kitchen".into()),
                None,
            ),
            "Kitchen Light",
        )
    }

    #[test]
    fn entity_id_is_domain_and_sanitised_object_id() {
        let id = entity_id(&light()).expect("id");
        assert_eq!(id.domain, "light");
        assert_eq!(id.object_id, "freeathome_abb700c12345_0");
    }

    #[test]
    fn on_off_state_mapping() {
        assert_eq!(on_off_state(Some("1")), "on");
        assert_eq!(on_off_state(Some("0")), "off");
        assert_eq!(on_off_state(None), "unknown");
    }

    #[test]
    fn register_sets_state_in_core() {
        let sm = StateMachine::new(EventBus::new());
        let d = light();
        let change = register(&sm, &d, Some("1")).expect("ok");
        assert!(change.is_some());
        let st = sm.get(&entity_id(&d).expect("id")).expect("state");
        assert_eq!(st.state, "on");
    }

    #[test]
    fn register_includes_friendly_name() {
        let sm = StateMachine::new(EventBus::new());
        let d = light();
        register(&sm, &d, Some("0")).expect("ok");
        let st = sm.get(&entity_id(&d).expect("id")).expect("state");
        assert_eq!(
            st.attributes.get("friendly_name").and_then(|v| v.as_str()),
            Some("Kitchen Light")
        );
    }
}
