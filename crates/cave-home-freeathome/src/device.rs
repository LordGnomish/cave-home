// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Device abstraction over a SysAP channel.

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_free_home::{
        Channel, ChannelId, DeviceKind, DeviceSerial, Function, Pairing, Value,
    };

    fn dev(function: Function, name: &str) -> Device {
        Device::new(
            DeviceSerial::parse("ABB700C12345").expect("serial"),
            Channel::new(ChannelId::new(0), function, Some("Living Room".into()), None),
            name,
        )
    }

    #[test]
    fn dimmer_is_a_controllable_light() {
        let d = dev(Function::DimmingActuator, "Lamp");
        assert_eq!(d.kind(), DeviceKind::Light);
        assert!(d.is_controllable());
    }

    #[test]
    fn switch_actuator_is_switch() {
        assert_eq!(dev(Function::SwitchActuator, "Plug").kind(), DeviceKind::Switch);
    }

    #[test]
    fn blind_is_cover() {
        assert_eq!(dev(Function::BlindActuator, "Shade").kind(), DeviceKind::Cover);
    }

    #[test]
    fn room_controller_is_climate() {
        assert_eq!(
            dev(Function::RoomTemperatureController, "Heat").kind(),
            DeviceKind::Climate
        );
    }

    #[test]
    fn scene_is_scene() {
        assert_eq!(dev(Function::Scene, "Movie").kind(), DeviceKind::Scene);
    }

    #[test]
    fn sensor_is_read_only() {
        let d = dev(Function::SwitchSensor, "Button");
        assert_eq!(d.kind(), DeviceKind::Sensor);
        assert!(!d.is_controllable());
    }

    #[test]
    fn friendly_name_and_room() {
        let d = dev(Function::DimmingActuator, "Kitchen Light");
        assert_eq!(d.friendly_name(), "Kitchen Light");
        assert_eq!(d.room(), Some("Living Room"));
    }

    #[test]
    fn usable_as_trait_object() {
        let d: Box<dyn FreeAtHomeDevice> = Box::new(dev(Function::DimmingActuator, "L"));
        assert_eq!(d.kind(), DeviceKind::Light);
        assert_eq!(d.channel(), ChannelId::new(0));
        assert_eq!(d.serial().as_str(), "ABB700C12345");
    }

    #[test]
    fn set_command_builds_brightness() {
        let d = dev(Function::DimmingActuator, "Lamp");
        let cmd = d
            .set_command(Pairing::SetBrightness, Value::percent(50).expect("pct"))
            .expect("command");
        assert_eq!(cmd.wire_value(), "50");
        assert_eq!(cmd.pairing(), Pairing::SetBrightness);
    }

    #[test]
    fn set_command_rejects_unsupported_pairing() {
        let d = dev(Function::DimmingActuator, "Lamp");
        let r = d.set_command(
            Pairing::SetTargetTemperature,
            Value::temperature(21.0).expect("temp"),
        );
        assert!(r.is_err());
    }

    #[test]
    fn writable_pairings_for_dimmer() {
        let d = dev(Function::DimmingActuator, "Lamp");
        assert!(d.supports(Pairing::SetBrightness));
        assert!(d.supports(Pairing::SwitchOnOff));
        assert!(!d.supports(Pairing::SetBlindPosition));
    }

    #[test]
    fn sensor_supports_nothing_writable() {
        let d = dev(Function::SwitchSensor, "Button");
        assert!(!d.supports(Pairing::SwitchOnOff));
    }
}
