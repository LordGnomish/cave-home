// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Portal admin → Hue surfaces.
//!
//! Two sub-pages live behind `/admin/hue/`:
//!
//! - `/admin/hue/integration/*` — the cave-home-hue *client* path (talk to
//!   a physical Hue Bridge). Grandma-friendly: "Hue Bridge'i bul" ->
//!   "Tuşa basın" -> "<N> lamba bulundu". No raw IPs, UUIDs, app keys.
//! - `/admin/hue/bridge-emu/*` — the cave-home-hue-bridge-emu *emulator*
//!   path. **Advanced-mode only** per ADR-010 / ADR-007: rendered only
//!   when the Developer-view toggle is on.
//!
//! This module ships the typed view-model + label catalogue + page-route
//! enum. The actual HTTP wiring is wired by `cave-home-binary`; the unit
//! tests below pin the Charter v6 ADR-007 grandma-friendly vocabulary so
//! drift triggers a CI failure rather than a UX regression.

/// Charter v6 grandma-friendly label catalogue. Reviewers verify these
/// strings never contain raw IPs / UUIDs / app-key fragments.
pub mod labels {
    pub const HEADING_INTEGRATION: &str = "Hue Bridge'e bağlan";
    pub const HEADING_BRIDGE_EMU: &str = "Bu cihazı Hue Bridge olarak yayınla";
    pub const ACTION_PAIR: &str = "Tuşa basın";
    pub const ACTION_FIND: &str = "Bul";
    pub const STATUS_SEARCHING: &str = "Hue Bridge aranıyor…";
    pub const STATUS_NO_BRIDGE: &str = "Hue Bridge bulunamadı.";
    pub const STATUS_PAIRED: &str = "Bağlandı";
    pub const STATUS_WAITING_BUTTON: &str = "Hue Bridge üzerindeki tuşa basın (30 saniye).";
    pub const LIGHT_NOUN: &str = "Lamba";
    pub const GROUP_NOUN: &str = "Oda";
    pub const SCENE_NOUN: &str = "Sahne";
    pub const SENSOR_NOUN: &str = "Sensör";
    pub const BUTTON_NOUN: &str = "Düğme";
    pub const EMU_ADVANCED_BADGE: &str = "Gelişmiş";
    pub const EMU_TOGGLE_ON_TEXT: &str = "Yayında";
    pub const EMU_TOGGLE_OFF_TEXT: &str = "Kapalı";
}

/// Top-level page routes under `/admin/hue/`. Used by `cave-home-binary`
/// to wire HTTP handlers; here just the typed enum + label tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// `/admin/hue/integration` — overview of any paired physical bridges.
    IntegrationIndex,
    /// `/admin/hue/integration/pair` — pairing wizard.
    IntegrationPair,
    /// `/admin/hue/integration/lights` — list of lights.
    IntegrationLights,
    /// `/admin/hue/integration/groups` — list of rooms / zones.
    IntegrationGroups,
    /// `/admin/hue/integration/scenes` — list of scenes.
    IntegrationScenes,
    /// `/admin/hue/integration/sensors` — list of motion / temperature / button sensors.
    IntegrationSensors,
    /// `/admin/hue/bridge-emu` — advanced-mode landing page.
    BridgeEmuIndex,
    /// `/admin/hue/bridge-emu/toggle` — POST endpoint flipping the emulator on/off.
    BridgeEmuToggle,
    /// `/admin/hue/bridge-emu/clients` — paired third-party Hue clients.
    BridgeEmuClients,
}

impl Page {
    /// Path segment used in `/admin/hue/...`. Used by tests + the binary.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::IntegrationIndex => "/admin/hue/integration",
            Self::IntegrationPair => "/admin/hue/integration/pair",
            Self::IntegrationLights => "/admin/hue/integration/lights",
            Self::IntegrationGroups => "/admin/hue/integration/groups",
            Self::IntegrationScenes => "/admin/hue/integration/scenes",
            Self::IntegrationSensors => "/admin/hue/integration/sensors",
            Self::BridgeEmuIndex => "/admin/hue/bridge-emu",
            Self::BridgeEmuToggle => "/admin/hue/bridge-emu/toggle",
            Self::BridgeEmuClients => "/admin/hue/bridge-emu/clients",
        }
    }
    /// True iff this page must be gated behind Settings → Developer view
    /// (ADR-010 §Charter §6.3 / ADR-007 compliance).
    #[must_use]
    pub const fn requires_developer_view(self) -> bool {
        matches!(
            self,
            Self::BridgeEmuIndex | Self::BridgeEmuToggle | Self::BridgeEmuClients
        )
    }
}

/// View-model for one row on `/admin/hue/integration/lights`.
///
/// Per ADR-007 the visible columns are Lamba (name) + Oda (parent group
/// name) + state ("Açık" / "Kapalı"). The raw bridge IP and resource UUID
/// are deliberately not represented here — they only appear behind the
/// developer-view JSON diagnostics page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightRow {
    pub name: String,
    pub room_name: String,
    /// True if on. Rendered as "Açık" / "Kapalı".
    pub is_on: bool,
}

/// View-model for one row on `/admin/hue/integration/groups`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    pub name: String,
    /// Number of lights in the room/zone.
    pub light_count: usize,
}

/// Bridge-emu status block shown on `/admin/hue/bridge-emu`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeEmuStatus {
    /// True iff the emulator is broadcasting.
    pub on_air: bool,
    /// Number of currently paired third-party app keys.
    pub paired_apps: usize,
    /// True iff there's an open link-button window right now.
    pub link_button_open: bool,
}

impl BridgeEmuStatus {
    /// Render the toggle-button label per Charter v6 vocabulary.
    #[must_use]
    pub fn toggle_label(&self) -> &'static str {
        if self.on_air {
            labels::EMU_TOGGLE_ON_TEXT
        } else {
            labels::EMU_TOGGLE_OFF_TEXT
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_pages_are_not_developer_gated() {
        for page in [
            Page::IntegrationIndex,
            Page::IntegrationPair,
            Page::IntegrationLights,
            Page::IntegrationGroups,
            Page::IntegrationScenes,
            Page::IntegrationSensors,
        ] {
            assert!(!page.requires_developer_view(), "{page:?} must be default");
        }
    }

    #[test]
    fn bridge_emu_pages_are_developer_gated() {
        for page in [
            Page::BridgeEmuIndex,
            Page::BridgeEmuToggle,
            Page::BridgeEmuClients,
        ] {
            assert!(page.requires_developer_view(), "{page:?} must be advanced");
        }
    }

    #[test]
    fn admin_routes_all_under_admin_hue() {
        for page in [
            Page::IntegrationIndex,
            Page::IntegrationPair,
            Page::IntegrationLights,
            Page::IntegrationGroups,
            Page::IntegrationScenes,
            Page::IntegrationSensors,
            Page::BridgeEmuIndex,
            Page::BridgeEmuToggle,
            Page::BridgeEmuClients,
        ] {
            assert!(
                page.path().starts_with("/admin/hue/"),
                "page {page:?} not under /admin/hue/"
            );
        }
    }

    #[test]
    fn grandma_labels_never_contain_raw_developer_jargon() {
        // ADR-007 — the headline persona never sees IPs / UUIDs / "app key".
        let banned = [
            "IP", "UUID", "app key", "appkey", "username", "REST", "JSON",
            "API",
        ];
        for label in [
            labels::HEADING_INTEGRATION,
            labels::ACTION_PAIR,
            labels::ACTION_FIND,
            labels::STATUS_SEARCHING,
            labels::STATUS_NO_BRIDGE,
            labels::STATUS_PAIRED,
            labels::STATUS_WAITING_BUTTON,
            labels::LIGHT_NOUN,
            labels::GROUP_NOUN,
            labels::SCENE_NOUN,
            labels::SENSOR_NOUN,
            labels::BUTTON_NOUN,
        ] {
            for term in &banned {
                assert!(
                    !label.to_lowercase().contains(&term.to_lowercase()),
                    "label `{label}` leaks developer jargon `{term}`"
                );
            }
        }
    }

    #[test]
    fn bridge_emu_label_carries_advanced_badge() {
        assert_eq!(labels::EMU_ADVANCED_BADGE, "Gelişmiş");
    }

    #[test]
    fn toggle_label_picks_correct_string() {
        let off = BridgeEmuStatus {
            on_air: false,
            paired_apps: 0,
            link_button_open: false,
        };
        assert_eq!(off.toggle_label(), labels::EMU_TOGGLE_OFF_TEXT);
        let on = BridgeEmuStatus {
            on_air: true,
            paired_apps: 3,
            link_button_open: false,
        };
        assert_eq!(on.toggle_label(), labels::EMU_TOGGLE_ON_TEXT);
    }
}
