// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The `ESPHome` entity data model.
//!
//! An `ESPHome` device exposes a list of *entities* — a light, a temperature
//! sensor, a relay switch. Each has an `object_id` (a stable slug) from which
//! the native API derives a 32-bit `key` via [`crate::hash::fnv1_hash`]; the
//! key is what state updates and commands reference on the wire.

use crate::hash::fnv1_hash;
use crate::message::MessageType;

/// The kind of an `ESPHome` entity. Each kind has a matching `ListEntities*` and
/// `*State` native-API message (see [`Self::list_response`] /
/// [`Self::state_response`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EntityKind {
    /// A read-only on/off input (motion, contact, presence…).
    BinarySensor,
    /// A blind, shade, garage door or other position-controlled cover.
    Cover,
    /// A fan.
    Fan,
    /// A light.
    Light,
    /// A numeric sensor (temperature, humidity, power…).
    Sensor,
    /// A controllable on/off output (relay, plug…).
    Switch,
    /// A free-text sensor.
    TextSensor,
}

impl EntityKind {
    /// The `ListEntities*Response` message that announces an entity of this
    /// kind during the entity-listing phase.
    #[must_use]
    pub const fn list_response(self) -> MessageType {
        match self {
            Self::BinarySensor => MessageType::ListEntitiesBinarySensorResponse,
            Self::Cover => MessageType::ListEntitiesCoverResponse,
            Self::Fan => MessageType::ListEntitiesFanResponse,
            Self::Light => MessageType::ListEntitiesLightResponse,
            Self::Sensor => MessageType::ListEntitiesSensorResponse,
            Self::Switch => MessageType::ListEntitiesSwitchResponse,
            Self::TextSensor => MessageType::ListEntitiesTextSensorResponse,
        }
    }

    /// The `*StateResponse` message that carries a state update for this kind.
    #[must_use]
    pub const fn state_response(self) -> MessageType {
        match self {
            Self::BinarySensor => MessageType::BinarySensorStateResponse,
            Self::Cover => MessageType::CoverStateResponse,
            Self::Fan => MessageType::FanStateResponse,
            Self::Light => MessageType::LightStateResponse,
            Self::Sensor => MessageType::SensorStateResponse,
            Self::Switch => MessageType::SwitchStateResponse,
            Self::TextSensor => MessageType::TextSensorStateResponse,
        }
    }
}

/// `ESPHome`'s `EntityCategory` — how the front-end should file an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum EntityCategory {
    /// Ordinary, primary entity (`0`).
    #[default]
    None,
    /// A configuration control (`1`).
    Config,
    /// A diagnostic readout (`2`).
    Diagnostic,
}

impl EntityCategory {
    /// The numeric `entity_category` value sent on the wire.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Config => 1,
            Self::Diagnostic => 2,
        }
    }

    /// The category for a wire value, or `None` if out of range.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::None),
            1 => Some(Self::Config),
            2 => Some(Self::Diagnostic),
            _ => None,
        }
    }
}

/// A single entity advertised by a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityInfo {
    /// What kind of entity this is.
    pub kind: EntityKind,
    /// The stable slug the device assigns; the [`Self::key`] is derived from it.
    pub object_id: String,
    /// The human-facing name ("Living Room Light").
    pub name: String,
    /// How the front-end should file the entity.
    pub category: EntityCategory,
}

impl EntityInfo {
    /// Build an entity with the default (`None`) category.
    pub fn new(kind: EntityKind, object_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind,
            object_id: object_id.into(),
            name: name.into(),
            category: EntityCategory::None,
        }
    }

    /// The native-API `key` for this entity: the FNV-1 hash of its `object_id`.
    #[must_use]
    pub fn key(&self) -> u32 {
        fnv1_hash(&self.object_id)
    }
}
