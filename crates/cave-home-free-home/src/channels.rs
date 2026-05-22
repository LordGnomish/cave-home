// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/channels/base.py
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/channels/switch_actuator.py
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/channels/dimming_actuator.py
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/channels/cover_actuator.py
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/channels/temperature_sensor.py
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! free@home channel model.
//!
//! Upstream uses a class hierarchy: `Base` → `SimpleSwitchActuator` →
//! `SwitchActuator`, `DimmingActuator`, `CoverActuator`,
//! `TemperatureSensor`, … We map the same hierarchy onto a Rust enum
//! [`Channel`] + a shared [`ChannelBase`] struct. Pattern matches in
//! cave-home portal/cli code dispatch on the [`ChannelKind`] discriminant.

use std::collections::HashMap;

use crate::pairing::Pairing;

/// Common fields lifted out of upstream's `Base.__init__`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelBase {
    pub device_serial: String,
    pub channel_id: String,
    pub channel_name: String,
    pub inputs: HashMap<String, Datapoint>,
    pub outputs: HashMap<String, Datapoint>,
    pub floor_name: Option<String>,
    pub room_name: Option<String>,
}

/// Single SysAP datapoint (input or output of a channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datapoint {
    pub pairing_id: u32,
    pub value: Option<String>,
}

impl ChannelBase {
    /// Find the (id, datapoint) pair whose `pairingID` matches.
    ///
    /// Equivalent to upstream `Base.get_input_by_pairing()` /
    /// `get_output_by_pairing()`.
    #[must_use]
    pub fn input_by_pairing(&self, pairing: Pairing) -> Option<(&str, &Datapoint)> {
        self.inputs
            .iter()
            .find(|(_, dp)| dp.pairing_id == pairing.value())
            .map(|(k, dp)| (k.as_str(), dp))
    }

    #[must_use]
    pub fn output_by_pairing(&self, pairing: Pairing) -> Option<(&str, &Datapoint)> {
        self.outputs
            .iter()
            .find(|(_, dp)| dp.pairing_id == pairing.value())
            .map(|(k, dp)| (k.as_str(), dp))
    }

    /// Apply a websocket datapoint delta — returns `true` if the
    /// datapoint matched something on this channel.
    pub fn apply_delta(&mut self, datapoint_id: &str, value: &str) -> bool {
        if let Some(dp) = self.inputs.get_mut(datapoint_id) {
            dp.value = Some(value.to_string());
            return true;
        }
        if let Some(dp) = self.outputs.get_mut(datapoint_id) {
            dp.value = Some(value.to_string());
            return true;
        }
        false
    }
}

// ---------- SwitchActuator ----------

/// `SwitchActuator` — binary on/off light or socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchActuator {
    pub base: ChannelBase,
    pub state: Option<bool>,
}

impl SwitchActuator {
    pub fn refresh_state(&mut self) {
        if let Some((_, dp)) = self.base.output_by_pairing(Pairing::AlInfoOnOff) {
            self.state = dp.value.as_deref().map(|v| v == "1");
        }
    }

    /// Switch input id + the value to publish — caller wires the actual
    /// REST call. Mirrors upstream's `_set_switching_datapoint`.
    pub fn turn_on_command(&self) -> Option<SetDatapointCommand> {
        self.set_switching(true)
    }

    pub fn turn_off_command(&self) -> Option<SetDatapointCommand> {
        self.set_switching(false)
    }

    fn set_switching(&self, on: bool) -> Option<SetDatapointCommand> {
        let (id, _) = self.base.input_by_pairing(Pairing::AlSwitchOnOff)?;
        Some(SetDatapointCommand {
            device_serial: self.base.device_serial.clone(),
            channel_id: self.base.channel_id.clone(),
            datapoint: id.to_string(),
            value: if on { "1" } else { "0" }.to_string(),
        })
    }
}

// ---------- DimmingActuator ----------

/// `DimmingActuator` — dimmable light, 0..=100 % brightness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimmingActuator {
    pub base: ChannelBase,
    pub state: Option<bool>,
    pub brightness: Option<u8>,
}

impl DimmingActuator {
    pub fn refresh_state(&mut self) {
        if let Some((_, dp)) = self.base.output_by_pairing(Pairing::AlInfoOnOff) {
            self.state = dp.value.as_deref().map(|v| v == "1");
        }
        if let Some((_, dp)) = self
            .base
            .output_by_pairing(Pairing::AlInfoActualDimmingValue)
        {
            self.brightness = dp.value.as_deref().and_then(|v| v.parse().ok());
        }
    }

    pub fn set_brightness_command(&self, value: u8) -> Option<SetDatapointCommand> {
        // Upstream clamps to 1..=100.
        let v = value.clamp(1, 100);
        let (id, _) = self.base.input_by_pairing(Pairing::AlAbsoluteSetValue)?;
        Some(SetDatapointCommand {
            device_serial: self.base.device_serial.clone(),
            channel_id: self.base.channel_id.clone(),
            datapoint: id.to_string(),
            value: v.to_string(),
        })
    }
}

// ---------- CoverActuator ----------

/// `CoverActuator` — blind / shutter / awning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverState {
    Unknown,
    Opened,
    PartlyOpened,
    Opening,
    Closing,
}

impl CoverState {
    /// Match upstream's `CoverActuatorState` mapping.
    #[must_use]
    pub fn from_value(value: &str) -> Self {
        match value {
            "0" => Self::Opened,
            "1" => Self::PartlyOpened,
            "2" => Self::Opening,
            "3" => Self::Closing,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverActuator {
    pub base: ChannelBase,
    pub state: CoverState,
    pub position: Option<u8>,
}

impl CoverActuator {
    pub fn refresh_state(&mut self) {
        if let Some((_, dp)) = self.base.output_by_pairing(Pairing::AlInfoMoveUpDown) {
            self.state = dp
                .value
                .as_deref()
                .map(CoverState::from_value)
                .unwrap_or(CoverState::Unknown);
        }
        if let Some((_, dp)) = self
            .base
            .output_by_pairing(Pairing::AlCurrentAbsolutePositionBlindsPercentage)
        {
            self.position = dp.value.as_deref().and_then(|v| v.parse().ok());
        }
    }

    pub fn open_command(&self) -> Option<SetDatapointCommand> {
        self.set_moving("0")
    }

    pub fn close_command(&self) -> Option<SetDatapointCommand> {
        self.set_moving("1")
    }

    fn set_moving(&self, value: &str) -> Option<SetDatapointCommand> {
        let (id, _) = self.base.input_by_pairing(Pairing::AlMoveUpDown)?;
        Some(SetDatapointCommand {
            device_serial: self.base.device_serial.clone(),
            channel_id: self.base.channel_id.clone(),
            datapoint: id.to_string(),
            value: value.to_string(),
        })
    }
}

// ---------- TemperatureSensor ----------

/// Outdoor temperature sensor.
#[derive(Debug, Clone, PartialEq)]
pub struct TemperatureSensor {
    pub base: ChannelBase,
    pub state: Option<f64>,
    pub alarm: Option<bool>,
}

impl TemperatureSensor {
    pub fn refresh_state(&mut self) {
        if let Some((_, dp)) = self.base.output_by_pairing(Pairing::AlOutdoorTemperature) {
            self.state = dp.value.as_deref().and_then(|v| v.parse().ok());
        }
        if let Some((_, dp)) = self.base.output_by_pairing(Pairing::AlFrostAlarm) {
            self.alarm = dp.value.as_deref().map(|v| v == "1");
        }
    }
}

// ---------- Channel sum type ----------

/// Tag identifying the runtime variant of a [`Channel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    Switch,
    Dimmer,
    Cover,
    TemperatureSensor,
}

/// The free@home channel sum type. Pattern-match callers dispatch on
/// this enum; the wrapped `ChannelBase` is identical to upstream.
#[derive(Debug, Clone, PartialEq)]
pub enum Channel {
    Switch(SwitchActuator),
    Dimmer(DimmingActuator),
    Cover(CoverActuator),
    TemperatureSensor(TemperatureSensor),
}

impl Channel {
    #[must_use]
    pub fn kind(&self) -> ChannelKind {
        match self {
            Self::Switch(_) => ChannelKind::Switch,
            Self::Dimmer(_) => ChannelKind::Dimmer,
            Self::Cover(_) => ChannelKind::Cover,
            Self::TemperatureSensor(_) => ChannelKind::TemperatureSensor,
        }
    }

    #[must_use]
    pub fn base(&self) -> &ChannelBase {
        match self {
            Self::Switch(c) => &c.base,
            Self::Dimmer(c) => &c.base,
            Self::Cover(c) => &c.base,
            Self::TemperatureSensor(c) => &c.base,
        }
    }

    /// Apply a websocket delta and re-derive cached state.
    pub fn apply_delta(&mut self, datapoint_id: &str, value: &str) -> bool {
        let touched = match self {
            Self::Switch(c) => c.base.apply_delta(datapoint_id, value),
            Self::Dimmer(c) => c.base.apply_delta(datapoint_id, value),
            Self::Cover(c) => c.base.apply_delta(datapoint_id, value),
            Self::TemperatureSensor(c) => c.base.apply_delta(datapoint_id, value),
        };
        if touched {
            match self {
                Self::Switch(c) => c.refresh_state(),
                Self::Dimmer(c) => c.refresh_state(),
                Self::Cover(c) => c.refresh_state(),
                Self::TemperatureSensor(c) => c.refresh_state(),
            }
        }
        touched
    }
}

/// Imperative datapoint command — what the `api` layer must `PUT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetDatapointCommand {
    pub device_serial: String,
    pub channel_id: String,
    pub datapoint: String,
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn switch_base() -> ChannelBase {
        let mut inputs = HashMap::new();
        inputs.insert(
            "idp0000".to_string(),
            Datapoint {
                pairing_id: Pairing::AlSwitchOnOff.value(),
                value: Some("0".into()),
            },
        );
        let mut outputs = HashMap::new();
        outputs.insert(
            "odp0000".to_string(),
            Datapoint {
                pairing_id: Pairing::AlInfoOnOff.value(),
                value: Some("0".into()),
            },
        );
        ChannelBase {
            device_serial: "ABB7F500BCFB".into(),
            channel_id: "ch0000".into(),
            channel_name: "Mutfak Tavan".into(),
            inputs,
            outputs,
            floor_name: Some("Zemin".into()),
            room_name: Some("Mutfak".into()),
        }
    }

    #[test]
    fn switch_turn_on_emits_command() {
        let sw = SwitchActuator {
            base: switch_base(),
            state: Some(false),
        };
        let cmd = sw.turn_on_command().expect("input present");
        assert_eq!(cmd.datapoint, "idp0000");
        assert_eq!(cmd.value, "1");
        assert_eq!(cmd.device_serial, "ABB7F500BCFB");
    }

    #[test]
    fn switch_refresh_decodes_state() {
        let mut sw = SwitchActuator {
            base: switch_base(),
            state: None,
        };
        sw.refresh_state();
        assert_eq!(sw.state, Some(false));

        sw.base
            .outputs
            .get_mut("odp0000")
            .expect("dp present")
            .value = Some("1".into());
        sw.refresh_state();
        assert_eq!(sw.state, Some(true));
    }

    #[test]
    fn dimmer_clamps_brightness() {
        let mut inputs = HashMap::new();
        inputs.insert(
            "idp0002".into(),
            Datapoint {
                pairing_id: Pairing::AlAbsoluteSetValue.value(),
                value: None,
            },
        );
        let base = ChannelBase {
            device_serial: "ABB".into(),
            channel_id: "ch01".into(),
            channel_name: "Dimmer".into(),
            inputs,
            outputs: HashMap::new(),
            floor_name: None,
            room_name: None,
        };
        let d = DimmingActuator {
            base,
            state: None,
            brightness: None,
        };
        assert_eq!(d.set_brightness_command(0).unwrap().value, "1");
        assert_eq!(d.set_brightness_command(50).unwrap().value, "50");
        assert_eq!(d.set_brightness_command(255).unwrap().value, "100");
    }

    #[test]
    fn cover_state_mapping() {
        assert_eq!(CoverState::from_value("0"), CoverState::Opened);
        assert_eq!(CoverState::from_value("1"), CoverState::PartlyOpened);
        assert_eq!(CoverState::from_value("2"), CoverState::Opening);
        assert_eq!(CoverState::from_value("3"), CoverState::Closing);
        assert_eq!(CoverState::from_value("X"), CoverState::Unknown);
    }

    #[test]
    fn cover_close_emits_value_1() {
        let mut inputs = HashMap::new();
        inputs.insert(
            "idp0000".into(),
            Datapoint {
                pairing_id: Pairing::AlMoveUpDown.value(),
                value: None,
            },
        );
        let base = ChannelBase {
            device_serial: "ABB".into(),
            channel_id: "ch01".into(),
            channel_name: "Salon Perde".into(),
            inputs,
            outputs: HashMap::new(),
            floor_name: None,
            room_name: None,
        };
        let cov = CoverActuator {
            base,
            state: CoverState::Unknown,
            position: None,
        };
        assert_eq!(cov.close_command().unwrap().value, "1");
        assert_eq!(cov.open_command().unwrap().value, "0");
    }

    #[test]
    fn temperature_sensor_parses_float() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "odp0000".into(),
            Datapoint {
                pairing_id: Pairing::AlOutdoorTemperature.value(),
                value: Some("21.5".into()),
            },
        );
        let base = ChannelBase {
            device_serial: "ABB".into(),
            channel_id: "ch01".into(),
            channel_name: "Dış Sıcaklık".into(),
            inputs: HashMap::new(),
            outputs,
            floor_name: None,
            room_name: None,
        };
        let mut s = TemperatureSensor {
            base,
            state: None,
            alarm: None,
        };
        s.refresh_state();
        assert_eq!(s.state, Some(21.5));
    }

    #[test]
    fn channel_apply_delta_routes_to_inputs_and_outputs() {
        let mut ch = Channel::Switch(SwitchActuator {
            base: switch_base(),
            state: Some(false),
        });
        assert!(ch.apply_delta("odp0000", "1"));
        if let Channel::Switch(sw) = &ch {
            assert_eq!(sw.state, Some(true));
        }
        assert!(!ch.apply_delta("nope", "0"));
    }
}
