// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The free@home portal viewmodel.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::Device;
    use cave_home_free_home::{Channel, ChannelId, DeviceKind, DeviceSerial, Function, Lang};

    fn device(function: Function, name: &str) -> Device {
        Device::new(
            DeviceSerial::parse("ABB700C12345").expect("serial"),
            Channel::new(ChannelId::new(0), function, Some("Kitchen".into()), None),
            name,
        )
    }

    #[test]
    fn tile_carries_identity_and_state() {
        let d = device(Function::DimmingActuator, "Kitchen Light");
        let t = tile(&d, "on").expect("tile");
        assert_eq!(t.entity_id, "light.freeathome_abb700c12345_0");
        assert_eq!(t.name, "Kitchen Light");
        assert_eq!(t.kind, DeviceKind::Light);
        assert_eq!(t.room.as_deref(), Some("Kitchen"));
        assert_eq!(t.state, "on");
        assert!(t.controllable);
    }

    #[test]
    fn dimmer_controls_include_toggle_and_brightness() {
        let c = controls(&device(Function::DimmingActuator, "Lamp"));
        assert!(c.contains(&Control::Toggle));
        assert!(c.contains(&Control::Brightness));
    }

    #[test]
    fn cover_controls_include_position() {
        let c = controls(&device(Function::BlindActuator, "Shade"));
        assert!(c.contains(&Control::Position));
    }

    #[test]
    fn scene_control_is_activate() {
        let c = controls(&device(Function::Scene, "Movie"));
        assert_eq!(c, vec![Control::Activate]);
    }

    #[test]
    fn detail_bundles_tile_and_controls() {
        let d = device(Function::DimmingActuator, "Lamp");
        let view = detail(&d, "off").expect("detail");
        assert_eq!(view.tile.state, "off");
        assert!(view.controls.contains(&Control::Brightness));
    }

    #[test]
    fn kind_label_is_localised() {
        assert_eq!(kind_label(DeviceKind::Switch, Lang::En), "Switch");
        assert_eq!(kind_label(DeviceKind::Switch, Lang::De), "Schalter");
        assert_eq!(kind_label(DeviceKind::Switch, Lang::Tr), "Anahtar");
        assert_eq!(kind_label(DeviceKind::Sensor, Lang::Tr), "Sensör");
    }

    #[test]
    fn sensors_filters_only_sensors() {
        let light = tile(&device(Function::DimmingActuator, "Lamp"), "on").expect("tile");
        let sensor = tile(&device(Function::SwitchSensor, "Button"), "off").expect("tile");
        let only = sensors(&[light, sensor]);
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].kind, DeviceKind::Sensor);
    }
}
