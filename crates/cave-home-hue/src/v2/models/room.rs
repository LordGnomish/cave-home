// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/models/room.py
//! v2 Room resource. Mirrors `aiohue.v2.models.room`.

use crate::v2::models::resource::{ResourceIdentifier, ResourceTypes};
use serde::{Deserialize, Serialize};

/// `aiohue.v2.models.room.RoomArchetype`. Hue archetypes drive default
/// icons in the official app; we round-trip the value as a string so any
/// archetype Philips ships in firmware survives without code changes.
pub type RoomArchetype = String;

/// `aiohue.v2.models.room.RoomMetadata`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomMetadata {
    pub name: String,
    pub archetype: RoomArchetype,
}

/// `aiohue.v2.models.room.Room`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_v1: Option<String>,
    pub children: Vec<ResourceIdentifier>,
    pub services: Vec<ResourceIdentifier>,
    pub metadata: RoomMetadata,
    #[serde(default = "default_room_type", rename = "type")]
    pub type_: ResourceTypes,
}

const fn default_room_type() -> ResourceTypes {
    ResourceTypes::Room
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn room_decodes_with_archetype_and_children() {
        let r: Room = serde_json::from_value(json!({
            "id": "room-1",
            "children": [{"rid": "dev-1", "rtype": "device"}],
            "services": [{"rid": "grouped-light-1", "rtype": "grouped_light"}],
            "metadata": {"name": "Salon", "archetype": "living_room"},
            "type": "room"
        }))
        .unwrap();
        assert_eq!(r.metadata.name, "Salon");
        assert_eq!(r.metadata.archetype, "living_room");
        assert_eq!(r.children.len(), 1);
    }
}
