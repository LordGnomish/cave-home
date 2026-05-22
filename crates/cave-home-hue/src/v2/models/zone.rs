// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/models/zone.py
//! v2 Zone resource. Mirrors `aiohue.v2.models.zone`.
//!
//! A Zone groups light *services* (not devices) so a single light service
//! can sit in multiple zones. The shape is otherwise identical to a Room.

use crate::v2::models::resource::{ResourceIdentifier, ResourceTypes};
use crate::v2::models::room::RoomMetadata;
use serde::{Deserialize, Serialize};

/// `aiohue.v2.models.zone.Zone`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_v1: Option<String>,
    pub children: Vec<ResourceIdentifier>,
    pub services: Vec<ResourceIdentifier>,
    pub metadata: RoomMetadata,
    #[serde(default = "default_zone_type", rename = "type")]
    pub type_: ResourceTypes,
}

const fn default_zone_type() -> ResourceTypes {
    ResourceTypes::Zone
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn zone_decodes_with_archetype_and_children() {
        let z: Zone = serde_json::from_value(json!({
            "id": "zone-1",
            "children": [{"rid": "light-svc-1", "rtype": "light"}],
            "services": [{"rid": "grouped-light-1", "rtype": "grouped_light"}],
            "metadata": {"name": "Üst kat", "archetype": "upstairs"},
            "type": "zone"
        }))
        .unwrap();
        assert_eq!(z.metadata.name, "Üst kat");
    }
}
