// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Projecting a free@home channel onto a uniform cave-home device kind.
//!
//! The rest of the hub does not know or care that a light came from free@home,
//! Zigbee or Matter — it sees a [`DeviceKind`]. This module is the single
//! translation point: a channel's [`crate::function::Function`] decides which
//! kind it becomes, and [`channel_kind`] does the lookup against the parsed
//! topology so callers never touch raw function IDs.

use crate::function::Function;
use crate::topology::Channel;

/// A uniform cave-home device kind. Every integration projects onto this set so
/// automations, the Portal and voice replies treat all vendors identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// A controllable light (on/off and/or brightness).
    Light,
    /// A blind, shutter or awning.
    Cover,
    /// A thermostat / room-temperature controller.
    Climate,
    /// A binary on/off actuator that is not a light (e.g. a socket).
    Switch,
    /// A saved scene.
    Scene,
    /// A read-only sensor.
    Sensor,
}

impl DeviceKind {
    /// A short, stable lowercase tag (used in the developer view / logs, never
    /// shown to a household).
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Cover => "cover",
            Self::Climate => "climate",
            Self::Switch => "switch",
            Self::Scene => "scene",
            Self::Sensor => "sensor",
        }
    }
}

/// The device kind a channel projects onto.
#[must_use]
pub const fn channel_kind(channel: &Channel) -> DeviceKind {
    channel.function().device_kind()
}

/// The device kind a bare function projects onto (convenience).
#[must_use]
pub const fn function_kind(function: Function) -> DeviceKind {
    function.device_kind()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::Channel;

    #[test]
    fn function_kind_covers_every_function() {
        assert_eq!(
            function_kind(Function::DimmingActuator),
            DeviceKind::Light
        );
        assert_eq!(function_kind(Function::BlindActuator), DeviceKind::Cover);
        assert_eq!(
            function_kind(Function::RoomTemperatureController),
            DeviceKind::Climate
        );
        assert_eq!(function_kind(Function::SwitchActuator), DeviceKind::Switch);
        assert_eq!(function_kind(Function::Scene), DeviceKind::Scene);
    }

    #[test]
    fn channel_kind_reads_from_topology() {
        let ch = Channel::new(
            crate::id::ChannelId::new(3),
            Function::BlindActuator,
            Some("Living room".into()),
            Some("Ground floor".into()),
        );
        assert_eq!(channel_kind(&ch), DeviceKind::Cover);
    }

    #[test]
    fn tags_are_unique_and_lowercase() {
        let kinds = [
            DeviceKind::Light,
            DeviceKind::Cover,
            DeviceKind::Climate,
            DeviceKind::Switch,
            DeviceKind::Scene,
            DeviceKind::Sensor,
        ];
        let mut tags: Vec<&str> = kinds.iter().map(|k| k.tag()).collect();
        let n = tags.len();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), n, "tags must be unique");
        assert!(kinds.iter().all(|k| k.tag().chars().all(|c| c.is_ascii_lowercase())));
    }
}
