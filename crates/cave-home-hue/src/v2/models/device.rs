// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/models/device.py
//! v2 Device resource. Mirrors `aiohue.v2.models.device`.
//!
//! Reference: <https://developers.meethue.com/develop/hue-api-v2/api-reference/#resource_device>.

use crate::v2::models::resource::{ResourceIdentifier, ResourceTypes};
use serde::{Deserialize, Serialize};

/// `aiohue.v2.models.device.ProductData`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductData {
    pub model_id: String,
    pub manufacturer_name: String,
    pub product_name: String,
    pub product_archetype: String,
    pub certified: bool,
    pub software_version: String,
    #[serde(default)]
    pub hardware_platform_type: Option<String>,
}

/// `aiohue.v2.models.device.DeviceMetaData`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceMetaData {
    pub archetype: String,
    pub name: String,
}

/// `aiohue.v2.models.device.Device`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_v1: Option<String>,
    pub product_data: ProductData,
    pub metadata: DeviceMetaData,
    /// All resource services this device exposes (lights, motion, etc).
    pub services: Vec<ResourceIdentifier>,
    #[serde(default = "default_device_type", rename = "type")]
    pub type_: ResourceTypes,
}

const fn default_device_type() -> ResourceTypes {
    ResourceTypes::Device
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn device_decodes_signify_light() {
        let d: Device = serde_json::from_value(json!({
            "id": "dev-1",
            "product_data": {
                "model_id": "LCT012",
                "manufacturer_name": "Signify Netherlands B.V.",
                "product_name": "Hue color candle",
                "product_archetype": "candle_bulb",
                "certified": true,
                "software_version": "1.108.10"
            },
            "metadata": {"archetype": "candle_bulb", "name": "Mutfak Lambası"},
            "services": [
                {"rid": "light-1", "rtype": "light"},
                {"rid": "zigbee-1", "rtype": "zigbee_connectivity"}
            ],
            "type": "device"
        }))
        .unwrap();
        assert_eq!(d.product_data.model_id, "LCT012");
        assert_eq!(d.services.len(), 2);
    }
}
