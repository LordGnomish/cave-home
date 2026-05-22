// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/models/button.py
//! v2 Button (dimmer switch buttons, smart-button single button, ...).
//! Mirrors `aiohue.v2.models.button`.

use crate::v2::models::feature::ButtonReport;
use crate::v2::models::resource::{ResourceIdentifier, ResourceTypes};
use serde::{Deserialize, Serialize};

/// `aiohue.v2.models.button.ButtonFeature`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonFeature {
    #[serde(default)]
    pub last_event: Option<String>,
    #[serde(default)]
    pub button_report: Option<ButtonReport>,
    #[serde(default)]
    pub repeat_interval: Option<u32>,
    #[serde(default)]
    pub event_values: Option<Vec<String>>,
}

/// `aiohue.v2.models.button.ButtonMetadata`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonMetadata {
    pub control_id: u8,
}

/// `aiohue.v2.models.button.Button`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Button {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_v1: Option<String>,
    pub owner: ResourceIdentifier,
    pub metadata: ButtonMetadata,
    pub button: ButtonFeature,
    #[serde(default = "default_button_type", rename = "type")]
    pub type_: ResourceTypes,
}

const fn default_button_type() -> ResourceTypes {
    ResourceTypes::Button
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::models::feature::ButtonReportEvent;
    use serde_json::json;

    #[test]
    fn button_decodes_with_report() {
        let b: Button = serde_json::from_value(json!({
            "id": "btn-1",
            "owner": {"rid": "dev-1", "rtype": "device"},
            "metadata": {"control_id": 1},
            "button": {
                "button_report": {"updated": "2026-05-17T20:00:00Z", "event": "short_release"}
            },
            "type": "button"
        }))
        .unwrap();
        assert_eq!(b.metadata.control_id, 1);
        let rep = b.button.button_report.unwrap();
        assert_eq!(rep.event, ButtonReportEvent::ShortRelease);
    }
}
