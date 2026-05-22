// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Portal admin surface for KNX-IP (cave-home-knx backing).
//!
//! Charter v6 / ADR-007 grandma-friendly UX: KNX physical addresses
//! (`1/2/3`), individual addresses (`1.1.5`), datapoint type identifiers
//! (`DPT 9.001`) and the routing multicast endpoint never appear in the
//! default view. They are only revealed when the user toggles Developer
//! view in `/admin/knx/dev`.

/// Localised grandma-facing labels used by the Lovelace-class panel.
pub mod labels {
    pub const SECTION_TITLE_TR: &str = "Bina Otomasyonu";
    pub const SECTION_TITLE_DE: &str = "Gebäudeautomation";
    pub const LIGHT: &str = "Işık";
    pub const COVER: &str = "Perde";
    pub const CLIMATE: &str = "Klima";
    pub const SENSOR: &str = "Sensör";
    pub const SCENE: &str = "Sahne";
    pub const HUB: &str = "Hub";
}

#[must_use]
pub fn admin_routes_placeholder() -> &'static str {
    "/admin/knx — Phase 2b"
}

/// Available REST routes the Phase 2b portal wires onto cave-home-knx.
#[must_use]
pub fn rest_routes() -> Vec<&'static str> {
    vec![
        "/admin/knx/bus",
        "/admin/knx/group",
        "/admin/knx/monitor",
        "/admin/knx/devices",
        "/admin/knx/tunnel/status",
    ]
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_routes_path_is_stable() {
        assert_eq!(admin_routes_placeholder(), "/admin/knx — Phase 2b");
    }

    #[test]
    fn grandma_labels_never_mention_knx_addresses() {
        for label in [
            labels::SECTION_TITLE_TR,
            labels::LIGHT,
            labels::COVER,
            labels::CLIMATE,
            labels::SENSOR,
            labels::SCENE,
            labels::HUB,
        ] {
            assert!(!label.contains("KNX"), "label {label} leaks bus name");
            assert!(!label.contains("DPT"), "label {label} leaks raw DPT type");
            assert!(!label.contains('/'), "label {label} looks like a group address");
            assert!(!label.contains("224.0"), "label {label} leaks multicast endpoint");
        }
    }

    #[test]
    fn rest_routes_include_monitor_and_tunnel() {
        let routes = rest_routes();
        assert!(routes.iter().any(|r| r.contains("monitor")));
        assert!(routes.iter().any(|r| r.contains("tunnel")));
        assert!(routes.iter().any(|r| r.contains("group")));
    }
}
