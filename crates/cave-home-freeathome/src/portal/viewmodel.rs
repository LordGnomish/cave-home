// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The free@home portal viewmodel.

use cave_home_free_home::{DeviceKind, Lang, Pairing};

use crate::core_bridge::entity_id;
use crate::device::{writable_pairings, FreeAtHomeDevice};
use crate::error::Result;

/// A control a household can operate on a device's detail page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// On/off toggle.
    Toggle,
    /// Brightness slider (0–100 %).
    Brightness,
    /// Blind/shutter position slider (0–100 %).
    Position,
    /// Temperature setpoint.
    Temperature,
    /// One-tap scene activation.
    Activate,
}

/// A single device as shown on a list page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceTile {
    /// The cave-home entity id (`light.freeathome_…`).
    pub entity_id: String,
    /// The household-facing name.
    pub name: String,
    /// The grandma-friendly kind.
    pub kind: DeviceKind,
    /// The room, if assigned.
    pub room: Option<String>,
    /// The current state token / value.
    pub state: String,
    /// Whether the device can be controlled.
    pub controllable: bool,
}

/// A device's detail page: its tile plus the controls it offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDetailView {
    /// The device tile.
    pub tile: DeviceTile,
    /// The controls to render.
    pub controls: Vec<Control>,
}

/// Build a list tile for a device given its current state token.
pub fn tile(device: &dyn FreeAtHomeDevice, state: &str) -> Result<DeviceTile> {
    Ok(DeviceTile {
        entity_id: entity_id(device)?.to_string(),
        name: device.friendly_name().to_string(),
        kind: device.kind(),
        room: device.room().map(ToString::to_string),
        state: state.to_string(),
        controllable: device.is_controllable(),
    })
}

/// The controls a device offers, derived from its writable pairings.
pub fn controls(device: &dyn FreeAtHomeDevice) -> Vec<Control> {
    let is_scene = device.kind() == DeviceKind::Scene;
    let mut out: Vec<Control> = Vec::new();
    for pairing in writable_pairings(device.function()) {
        let control = match pairing {
            Pairing::SwitchOnOff if is_scene => Some(Control::Activate),
            Pairing::SwitchOnOff => Some(Control::Toggle),
            Pairing::SetBrightness => Some(Control::Brightness),
            Pairing::SetBlindPosition => Some(Control::Position),
            Pairing::SetTargetTemperature => Some(Control::Temperature),
            // Move up/down is subsumed by the position slider.
            _ => None,
        };
        if let Some(c) = control {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// Build a device's detail view.
pub fn detail(device: &dyn FreeAtHomeDevice, state: &str) -> Result<DeviceDetailView> {
    Ok(DeviceDetailView {
        tile: tile(device, state)?,
        controls: controls(device),
    })
}

/// Filter a set of tiles down to sensors.
pub fn sensors(tiles: &[DeviceTile]) -> Vec<DeviceTile> {
    tiles
        .iter()
        .filter(|t| t.kind == DeviceKind::Sensor)
        .cloned()
        .collect()
}

/// A jargon-free, localised label for a device kind.
pub const fn kind_label(kind: DeviceKind, lang: Lang) -> &'static str {
    match (kind, lang) {
        (DeviceKind::Light, Lang::En) => "Light",
        (DeviceKind::Light, Lang::De) => "Licht",
        (DeviceKind::Light, Lang::Tr) => "Işık",
        (DeviceKind::Cover, Lang::En) => "Blind",
        (DeviceKind::Cover, Lang::De) => "Rollladen",
        (DeviceKind::Cover, Lang::Tr) => "Panjur",
        (DeviceKind::Climate, Lang::En | Lang::De) => "Thermostat",
        (DeviceKind::Climate, Lang::Tr) => "Termostat",
        (DeviceKind::Switch, Lang::En) => "Switch",
        (DeviceKind::Switch, Lang::De) => "Schalter",
        (DeviceKind::Switch, Lang::Tr) => "Anahtar",
        (DeviceKind::Scene, Lang::En) => "Scene",
        (DeviceKind::Scene, Lang::De) => "Szene",
        (DeviceKind::Scene, Lang::Tr) => "Sahne",
        (DeviceKind::Sensor, Lang::En | Lang::De) => "Sensor",
        (DeviceKind::Sensor, Lang::Tr) => "Sensör",
    }
}

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
