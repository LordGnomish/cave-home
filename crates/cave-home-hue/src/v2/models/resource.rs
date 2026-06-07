// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/models/resource.py
//! v2 generic [`ResourceTypes`] enum + [`ResourceIdentifier`].
//!
//! `ResourceTypes` is exhaustive — every type that appears in
//! `clip-api.schema.json#/definitions/ResourceTypes` is listed, even ones
//! we don't yet have a dedicated model for. The `UNKNOWN` variant catches
//! anything Philips ships in a firmware update that we haven't tracked yet.

use serde::{Deserialize, Serialize};

/// Source: `aiohue.v2.models.resource.ResourceTypes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceTypes {
    Device,
    BridgeHome,
    Room,
    Zone,
    Light,
    Button,
    RelativeRotary,
    Temperature,
    LightLevel,
    Motion,
    Entertainment,
    GroupedLight,
    DevicePower,
    DeviceUpdate,
    IpConnectivity,
    ZigbeeBridgeConnectivity,
    ZigbeeConnectivity,
    ZgpConnectivity,
    RemoteAccess,
    Bridge,
    DeviceDiscovery,
    SystemUpdate,
    Scene,
    SmartScene,
    EntertainmentConfiguration,
    PublicImage,
    AuthV1,
    BehaviorScript,
    BehaviorInstance,
    Geofence,
    GeofenceClient,
    Depender,
    Homekit,
    Matter,
    MatterFabric,
    Contact,
    Tamper,
    CameraMotion,
    ConvenienceAreaMotion,
    SecurityAreaMotion,
    MotionAreaConfiguration,
    ServiceGroup,
    PrivateGroup,
    GroupedMotion,
    GroupedLightLevel,
    BellButton,
    #[serde(other)]
    Unknown,
}

/// Source: `aiohue.v2.models.resource.ResourceIdentifier`.
/// Matches `clip-api.schema.json#/definitions/ResourceIdentifierGet` (and
/// the Post/Put/Delete cousins, which share the same shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceIdentifier {
    /// Resource UUID. Mirrors upstream `rid`.
    pub rid: String,
    /// Resource type. Mirrors upstream `rtype`.
    pub rtype: ResourceTypes,
}

/// Types that, in the v2 CLIP model, are "sensor-shaped" — owned by a
/// `device`, expose a measurement / event surface. Used by the events
/// controller for routing.
///
/// Source: `aiohue.v2.models.resource.SENSOR_RESOURCE_TYPES`.
pub const SENSOR_RESOURCE_TYPES: &[ResourceTypes] = &[
    ResourceTypes::DevicePower,
    ResourceTypes::Button,
    ResourceTypes::GeofenceClient,
    ResourceTypes::LightLevel,
    ResourceTypes::Motion,
    ResourceTypes::ConvenienceAreaMotion,
    ResourceTypes::SecurityAreaMotion,
    ResourceTypes::RelativeRotary,
    ResourceTypes::Temperature,
    ResourceTypes::ZigbeeConnectivity,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_types_round_trip() {
        let original = ResourceTypes::GroupedLight;
        let s = serde_json::to_string(&original).unwrap();
        assert_eq!(s, "\"grouped_light\"");
        let back: ResourceTypes = serde_json::from_str(&s).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn unknown_resource_type_round_trips_to_unknown() {
        let back: ResourceTypes = serde_json::from_str("\"some_future_type\"").unwrap();
        assert_eq!(back, ResourceTypes::Unknown);
    }

    #[test]
    fn resource_identifier_round_trip() {
        let id = ResourceIdentifier {
            rid: "abc-123".into(),
            rtype: ResourceTypes::Light,
        };
        let s = serde_json::to_string(&id).unwrap();
        let back: ResourceIdentifier = serde_json::from_str(&s).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn sensor_resource_types_includes_known_sensors() {
        assert!(SENSOR_RESOURCE_TYPES.contains(&ResourceTypes::Button));
        assert!(SENSOR_RESOURCE_TYPES.contains(&ResourceTypes::Motion));
        assert!(SENSOR_RESOURCE_TYPES.contains(&ResourceTypes::Temperature));
    }
}
