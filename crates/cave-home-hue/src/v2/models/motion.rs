// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/models/motion.py
//! v2 Motion sensor. Mirrors `aiohue.v2.models.motion`.
//!
//! Reference: <https://developers.meethue.com/develop/hue-api-v2/api-reference/#resource_motion>

use crate::v2::models::feature::MotionReport;
use crate::v2::models::resource::{ResourceIdentifier, ResourceTypes};
use serde::{Deserialize, Serialize};

/// `aiohue.v2.models.motion.MotionSensingFeature`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotionSensingFeature {
    pub motion_valid: bool,
    pub motion_report: Option<MotionReport>,
}

/// `aiohue.v2.models.motion.Motion`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Motion {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_v1: Option<String>,
    pub owner: ResourceIdentifier,
    pub enabled: bool,
    pub motion: MotionSensingFeature,
    #[serde(default = "default_motion_type", rename = "type")]
    pub type_: ResourceTypes,
}

const fn default_motion_type() -> ResourceTypes {
    ResourceTypes::Motion
}

impl Motion {
    /// Convenience: is the sensor reporting motion right now? `None` if no
    /// report is available (sensor hasn't fired since boot).
    #[must_use]
    pub fn is_motion(&self) -> Option<bool> {
        self.motion.motion_report.as_ref().map(|r| r.motion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn motion_decodes_active_state() {
        let m: Motion = serde_json::from_value(json!({
            "id": "motion-1",
            "owner": {"rid": "device-1", "rtype": "device"},
            "enabled": true,
            "motion": {
                "motion_valid": true,
                "motion_report": {"changed": "2026-05-17T20:00:00.000Z", "motion": true}
            },
            "type": "motion"
        }))
        .unwrap();
        assert!(m.enabled);
        assert_eq!(m.is_motion(), Some(true));
    }

    #[test]
    fn motion_handles_no_report() {
        let m: Motion = serde_json::from_value(json!({
            "id": "motion-1",
            "owner": {"rid": "device-1", "rtype": "device"},
            "enabled": false,
            "motion": {"motion_valid": false, "motion_report": null},
            "type": "motion"
        }))
        .unwrap();
        assert!(!m.enabled);
        assert_eq!(m.is_motion(), None);
    }
}
