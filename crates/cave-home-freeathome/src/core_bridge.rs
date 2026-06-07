// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Bridge from free@home devices into the cave-home-core entity registry.

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
