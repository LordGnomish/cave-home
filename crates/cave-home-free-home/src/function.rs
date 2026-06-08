// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! free@home function IDs.
//!
//! A *function* describes what a channel **is** — a switch actuator, a dimmer,
//! a blind actuator, a room-temperature controller, a scene. Busch-Jaeger
//! publishes these as 16-bit "function IDs" (the `functionID` field in the
//! System Access Point's device tree). cave-home models a curated set of the
//! documented IDs: the ones a Phase-1 household actually controls. The numeric
//! values are protocol-stable identifiers, pinned here so the device tree maps
//! straight onto a typed [`Function`].
//!
//! The remaining ~150 documented function IDs (HVAC fan stages, weather
//! stations, media, access control, …) are deferred — adding one is a new enum
//! variant + a [`crate::mapping`] row, with no API churn.

use crate::mapping::DeviceKind;

/// A curated set of documented free@home function IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Function {
    /// Binary on/off actuator — a light or a socket.
    SwitchActuator = 0x0007,
    /// Dimming actuator — a brightness-controllable light.
    DimmingActuator = 0x0012,
    /// Blind / shutter / awning actuator.
    BlindActuator = 0x0061,
    /// Attic / roof-window blind actuator (same control surface as a blind).
    AtticBlindActuator = 0x0062,
    /// Room-temperature controller (a thermostat).
    RoomTemperatureController = 0x0023,
    /// A scene — a saved set of device states activated as one.
    Scene = 0x4800,
    /// A binary sensor (e.g. a wall switch sensor) — read-only.
    SwitchSensor = 0x0000,
}

impl Function {
    /// The protocol function ID as exposed in the device tree.
    #[must_use]
    pub const fn id(self) -> u16 {
        self as u16
    }

    /// Map a numeric function ID from the device tree to a [`Function`].
    #[must_use]
    pub const fn from_id(id: u16) -> Option<Self> {
        match id {
            0x0007 => Some(Self::SwitchActuator),
            0x0012 => Some(Self::DimmingActuator),
            0x0061 => Some(Self::BlindActuator),
            0x0062 => Some(Self::AtticBlindActuator),
            0x0023 => Some(Self::RoomTemperatureController),
            0x4800 => Some(Self::Scene),
            0x0000 => Some(Self::SwitchSensor),
            _ => None,
        }
    }

    /// The cave-home device kind this function projects onto. This is the
    /// single point where a free@home function becomes a uniform hub device.
    #[must_use]
    pub const fn device_kind(self) -> DeviceKind {
        match self {
            Self::SwitchActuator => DeviceKind::Switch,
            Self::DimmingActuator => DeviceKind::Light,
            Self::BlindActuator | Self::AtticBlindActuator => DeviceKind::Cover,
            Self::RoomTemperatureController => DeviceKind::Climate,
            Self::Scene => DeviceKind::Scene,
            Self::SwitchSensor => DeviceKind::Sensor,
        }
    }

    /// Whether this function is controllable (vs. read-only sensor).
    #[must_use]
    pub const fn is_controllable(self) -> bool {
        !matches!(self, Self::SwitchSensor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_ids_are_stable() {
        assert_eq!(Function::SwitchActuator.id(), 0x0007);
        assert_eq!(Function::DimmingActuator.id(), 0x0012);
        assert_eq!(Function::BlindActuator.id(), 0x0061);
        assert_eq!(Function::RoomTemperatureController.id(), 0x0023);
        assert_eq!(Function::Scene.id(), 0x4800);
    }

    #[test]
    fn from_id_round_trips_known_functions() {
        for f in [
            Function::SwitchActuator,
            Function::DimmingActuator,
            Function::BlindActuator,
            Function::AtticBlindActuator,
            Function::RoomTemperatureController,
            Function::Scene,
            Function::SwitchSensor,
        ] {
            assert_eq!(Function::from_id(f.id()), Some(f));
        }
    }

    #[test]
    fn from_id_unknown_is_none() {
        assert_eq!(Function::from_id(0xFFFF), None);
        assert_eq!(Function::from_id(0x1234), None);
    }

    #[test]
    fn device_kind_mapping() {
        assert_eq!(Function::SwitchActuator.device_kind(), DeviceKind::Switch);
        assert_eq!(Function::DimmingActuator.device_kind(), DeviceKind::Light);
        assert_eq!(Function::BlindActuator.device_kind(), DeviceKind::Cover);
        assert_eq!(
            Function::AtticBlindActuator.device_kind(),
            DeviceKind::Cover
        );
        assert_eq!(
            Function::RoomTemperatureController.device_kind(),
            DeviceKind::Climate
        );
        assert_eq!(Function::Scene.device_kind(), DeviceKind::Scene);
        assert_eq!(Function::SwitchSensor.device_kind(), DeviceKind::Sensor);
    }

    #[test]
    fn controllability() {
        assert!(Function::DimmingActuator.is_controllable());
        assert!(Function::Scene.is_controllable());
        assert!(!Function::SwitchSensor.is_controllable());
    }
}
