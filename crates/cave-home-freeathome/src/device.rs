// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Device abstraction over a SysAP channel.
//!
//! free@home exposes *channels* (one function on a physical device). cave-home
//! treats each controllable channel as a [`Device`] and projects it onto a
//! grandma-friendly [`DeviceKind`] so a free@home blind behaves like any other
//! cover. The brain (function → kind, value codec, command validation) lives in
//! [`cave_home_free_home`]; this layer adds identity (serial), a display name
//! and a capability map of which write actions a function accepts.

use cave_home_free_home::{
    Channel, ChannelId, DeviceKind, DeviceSerial, Function, Pairing, SetDatapoint, Value,
};

use crate::error::{FreeAtHomeError, Result};

/// The behaviour every free@home device exposes to the rest of the hub.
pub trait FreeAtHomeDevice {
    /// The owning device serial.
    fn serial(&self) -> &DeviceSerial;
    /// The channel this device maps to.
    fn channel(&self) -> ChannelId;
    /// The free@home function behind the device.
    fn function(&self) -> Function;
    /// The household-facing display name.
    fn friendly_name(&self) -> &str;

    /// The room the device is assigned to, if known.
    fn room(&self) -> Option<&str> {
        None
    }

    /// The grandma-friendly device kind.
    fn kind(&self) -> DeviceKind {
        self.function().device_kind()
    }

    /// Whether the device can be controlled (vs. a read-only sensor).
    fn is_controllable(&self) -> bool {
        self.function().is_controllable()
    }
}

/// A controllable (or observable) free@home channel.
#[derive(Debug, Clone)]
pub struct Device {
    serial: DeviceSerial,
    channel: Channel,
    name: String,
}

impl Device {
    /// Build a device from its serial, channel and display name.
    pub fn new(serial: DeviceSerial, channel: Channel, name: impl Into<String>) -> Self {
        Self {
            serial,
            channel,
            name: name.into(),
        }
    }

    /// The room the channel is assigned to, if known.
    pub fn room(&self) -> Option<&str> {
        self.channel.room()
    }

    /// Whether the device's function accepts writes to `pairing`.
    pub fn supports(&self, pairing: Pairing) -> bool {
        writable_pairings(self.channel.function()).contains(&pairing)
    }

    /// Build a validated write command for `pairing`/`value`.
    ///
    /// Fails if the device's function does not accept the pairing, or if
    /// free@home rejects the value shape/range or role.
    pub fn set_command(&self, pairing: Pairing, value: Value) -> Result<SetDatapoint> {
        if !self.supports(pairing) {
            return Err(FreeAtHomeError::Domain(format!(
                "{:?} does not accept {pairing:?}",
                self.channel.function()
            )));
        }
        SetDatapoint::new(self.serial.as_str(), self.channel.id(), pairing, value)
            .map_err(|e| FreeAtHomeError::Domain(e.to_string()))
    }

    /// The pairing role that reports this device's primary observable state.
    ///
    /// On/off-shaped kinds (light, switch, sensor) report via [`Pairing::InfoOnOff`];
    /// a cover via [`Pairing::InfoBlindPosition`]; a thermostat via
    /// [`Pairing::InfoCurrentTemperature`]. A scene reports no live state.
    const fn primary_info_pairing(&self) -> Option<Pairing> {
        match self.channel.function().device_kind() {
            DeviceKind::Light | DeviceKind::Switch | DeviceKind::Sensor => {
                Some(Pairing::InfoOnOff)
            }
            DeviceKind::Cover => Some(Pairing::InfoBlindPosition),
            DeviceKind::Climate => Some(Pairing::InfoCurrentTemperature),
            DeviceKind::Scene => None,
        }
    }

    /// The current wire value of the device's primary reported-state datapoint,
    /// as last seen in the channel's datapoints (e.g. `"1"`, `"50"`, `"21.5"`).
    pub fn primary_value(&self) -> Option<&str> {
        let pairing = self.primary_info_pairing()?;
        self.channel
            .datapoint_for(pairing)
            .and_then(|dp| dp.value.as_deref())
    }

    /// A household-facing state token for list/detail views.
    ///
    /// On/off-shaped kinds render `on`/`off`/`unknown`; analogue kinds (cover
    /// position, climate temperature) render the raw wire value (or `unknown`);
    /// a scene has no live state and renders `scene`.
    pub fn display_state(&self) -> String {
        match self.channel.function().device_kind() {
            DeviceKind::Light | DeviceKind::Switch | DeviceKind::Sensor => {
                crate::core_bridge::on_off_state(self.primary_value()).to_string()
            }
            DeviceKind::Scene => "scene".to_string(),
            DeviceKind::Cover | DeviceKind::Climate => {
                self.primary_value().unwrap_or("unknown").to_string()
            }
        }
    }
}

impl FreeAtHomeDevice for Device {
    fn serial(&self) -> &DeviceSerial {
        &self.serial
    }

    fn channel(&self) -> ChannelId {
        self.channel.id()
    }

    fn function(&self) -> Function {
        self.channel.function()
    }

    fn friendly_name(&self) -> &str {
        &self.name
    }

    fn room(&self) -> Option<&str> {
        self.channel.room()
    }
}

/// The writable pairing roles a given function accepts.
pub const fn writable_pairings(function: Function) -> &'static [Pairing] {
    match function {
        // A switch toggles on/off; a scene is "activated" via the same on write.
        Function::SwitchActuator | Function::Scene => &[Pairing::SwitchOnOff],
        Function::DimmingActuator => &[Pairing::SwitchOnOff, Pairing::SetBrightness],
        Function::BlindActuator | Function::AtticBlindActuator => {
            &[Pairing::MoveUpDown, Pairing::SetBlindPosition]
        }
        Function::RoomTemperatureController => &[Pairing::SetTargetTemperature],
        Function::SwitchSensor => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_free_home::{
        Channel, ChannelId, DeviceKind, DeviceSerial, Function, Pairing, Value,
    };

    fn dev(function: Function, name: &str) -> Device {
        Device::new(
            DeviceSerial::parse("ABB700C12345").expect("serial"),
            Channel::new(
                ChannelId::new(0),
                function,
                Some("Living Room".into()),
                None,
            ),
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
        assert_eq!(
            dev(Function::SwitchActuator, "Plug").kind(),
            DeviceKind::Switch
        );
    }

    #[test]
    fn blind_is_cover() {
        assert_eq!(
            dev(Function::BlindActuator, "Shade").kind(),
            DeviceKind::Cover
        );
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
