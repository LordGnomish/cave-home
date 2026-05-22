// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/api.py (websocket subset)
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! SysAP WebSocket envelope parsing.
//!
//! Realtime updates from the SysAP arrive as JSON messages of the form
//! ```json
//! {
//!   "00000000-0000-0000-0000-000000000000": {
//!     "datapoints": {
//!       "ABB7F500BCFB/ch0000/odp0000": "1"
//!     },
//!     "devicesAdded": [],
//!     "devicesRemoved": [],
//!     "scenesTriggered": {}
//!   }
//! }
//! ```
//! We parse it into a [`WsUpdate`] struct that the [`crate::freeathome`]
//! facade can apply to its `Device` cache.

use serde::Deserialize;
use std::collections::HashMap;

use crate::error::{FreeAtHomeError, Result};

/// Per-SysAP datapoint delta envelope.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct WsSysApUpdate {
    #[serde(default)]
    pub datapoints: HashMap<String, String>,
    #[serde(default, rename = "devicesAdded")]
    pub devices_added: Vec<String>,
    #[serde(default, rename = "devicesRemoved")]
    pub devices_removed: Vec<String>,
    #[serde(default, rename = "scenesTriggered")]
    pub scenes_triggered: HashMap<String, serde_json::Value>,
}

/// Top-level websocket update — `{ <sysap_uuid>: WsSysApUpdate }`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WsUpdate {
    pub by_sysap: HashMap<String, WsSysApUpdate>,
}

impl WsUpdate {
    /// Parse a JSON websocket message into a [`WsUpdate`].
    pub fn from_json(raw: &str) -> Result<Self> {
        let parsed: HashMap<String, WsSysApUpdate> = serde_json::from_str(raw)
            .map_err(|e| FreeAtHomeError::Json(e.to_string()))?;
        Ok(Self { by_sysap: parsed })
    }

    /// Yield `(device_serial, channel_id, datapoint, value)` quadruples.
    /// Datapoint keys arrive as `<serial>/<channel>/<datapoint>` —
    /// matches upstream `update()`.
    pub fn iter_datapoints(&self) -> impl Iterator<Item = (&str, &str, &str, &str)> + '_ {
        self.by_sysap.values().flat_map(|s| {
            s.datapoints.iter().filter_map(|(k, v)| {
                let mut parts = k.splitn(3, '/');
                let serial = parts.next()?;
                let channel = parts.next()?;
                let dp = parts.next()?;
                Some((serial, channel, dp, v.as_str()))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_datapoint_envelope() {
        let raw = r#"{
            "00000000-0000-0000-0000-000000000000": {
                "datapoints": {
                    "ABB7F500BCFB/ch0000/odp0000": "1"
                },
                "devicesAdded": [],
                "devicesRemoved": [],
                "scenesTriggered": {}
            }
        }"#;
        let u = WsUpdate::from_json(raw).unwrap();
        let quads: Vec<_> = u.iter_datapoints().collect();
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0], ("ABB7F500BCFB", "ch0000", "odp0000", "1"));
    }

    #[test]
    fn malformed_json_errors() {
        assert!(matches!(
            WsUpdate::from_json("not json"),
            Err(FreeAtHomeError::Json(_))
        ));
    }

    #[test]
    fn datapoints_missing_field_defaults_to_empty() {
        let raw = r#"{"sysap":{}}"#;
        let u = WsUpdate::from_json(raw).unwrap();
        assert!(u.iter_datapoints().next().is_none());
    }
}
