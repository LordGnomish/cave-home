// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Portal admin surface for free@home (cave-home-free-home backing).
//!
//! Charter v6 / ADR-007 grandma-friendly UX: labels are room/device-class
//! Turkish first, German fallback. Device serials, SysAP UUIDs and
//! pairing IDs live in developer view only.

/// Localised grandma-facing labels used by the Lovelace-class panel.
pub mod labels {
    /// Section title — `Akıllı Ev / Smarthome`.
    pub const SECTION_TITLE_TR: &str = "Akıllı Ev";
    pub const SECTION_TITLE_DE: &str = "Smarthome";
    /// Channel-kind labels (TR primary).
    pub const SWITCH: &str = "Işık";
    pub const DIMMER: &str = "Dimmer";
    pub const COVER: &str = "Perde";
    pub const TEMPERATURE: &str = "Sensör";
    pub const SCENE: &str = "Sahne";
    pub const HUB: &str = "Hub";
}

#[must_use]
pub fn admin_routes_placeholder() -> &'static str {
    "/admin/free-home — Phase 2b"
}

/// Available REST routes the Phase 2b portal wires onto cave-home-free-home.
#[must_use]
pub fn rest_routes() -> Vec<&'static str> {
    vec![
        "/admin/free-home/sysap",
        "/admin/free-home/devices",
        "/admin/free-home/channels",
        "/admin/free-home/scenes",
        "/admin/free-home/datapoint/{serial}/{channel}/{datapoint}",
    ]
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_routes_path_is_stable() {
        assert_eq!(admin_routes_placeholder(), "/admin/free-home — Phase 2b");
    }

    #[test]
    fn grandma_labels_never_mention_raw_serial() {
        for label in [
            labels::SECTION_TITLE_TR,
            labels::SWITCH,
            labels::DIMMER,
            labels::COVER,
            labels::TEMPERATURE,
            labels::SCENE,
            labels::HUB,
        ] {
            assert!(!label.contains("ABB"), "label {label} leaks device-serial prefix");
            assert!(!label.contains("ch00"), "label {label} leaks raw channel id");
            assert!(!label.contains("SysAP"), "label {label} leaks raw SysAP term");
            assert!(!label.contains("free@home"), "label {label} leaks vendor name");
        }
    }

    #[test]
    fn rest_routes_have_expected_shape() {
        let routes = rest_routes();
        assert!(routes.iter().any(|r| r.contains("sysap")));
        assert!(routes.iter().any(|r| r.contains("devices")));
        assert!(routes.iter().any(|r| r.contains("datapoint")));
    }
}
