// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Grandma-friendly rendering of live UniFi state for the CLI (Charter §6.3,
//! ADR-007).
//!
//! The API surfaces return the sibling crates' domain types; the CLI track turns
//! them into the plain EN/DE/TR lines a household reads. These functions are
//! pure and synchronous — the CLI fetches over the async client, then renders
//! here — so they are fully unit-testable without a runtime, and the exact same
//! strings are asserted in tests and shown to the user.

use std::fmt::Write as _;

use cave_home_unifi_network::{DeviceState, NetworkClient, NetworkDevice};
use cave_home_unifi_protect::ProtectCamera;

use crate::access::DoorStatus;

/// The display language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English.
    En,
    /// German.
    De,
    /// Turkish.
    Tr,
}

/// Render one network device as a household line, e.g. "• Salon switch (switch)
/// — online".
#[must_use]
pub fn device_line(device: &NetworkDevice, lang: Lang) -> String {
    let kind = device.kind().household_word();
    let state = match (device.state(), lang) {
        (DeviceState::Online, Lang::En | Lang::De) => "online",
        (DeviceState::Online, Lang::Tr) => "çevrimiçi",
        (DeviceState::Offline, Lang::En | Lang::De) => "offline",
        (DeviceState::Offline, Lang::Tr) => "çevrimdışı",
    };
    format!("• {} ({kind}) — {state}", device.name())
}

/// Render one network client as a household line.
#[must_use]
pub fn client_line(client: &NetworkClient, lang: Lang) -> String {
    let mut line = format!("• {}", client.name());
    if client.is_wireless() {
        if let Some(ssid) = client.connection().ssid() {
            let on = match lang {
                Lang::En => "on",
                Lang::De => "auf",
                Lang::Tr => "ağında",
            };
            let _ = write!(line, " — {on} {ssid}");
        }
    } else {
        let wired = match lang {
            Lang::En => "wired",
            Lang::De => "kabelgebunden",
            Lang::Tr => "kablolu",
        };
        let _ = write!(line, " — {wired}");
    }
    if client.is_blocked() {
        let blocked = match lang {
            Lang::En => "blocked",
            Lang::De => "gesperrt",
            Lang::Tr => "engelli",
        };
        let _ = write!(line, " [{blocked}]");
    }
    line
}

/// Render one door's live lock state as a household line.
#[must_use]
pub fn door_line(door: &DoorStatus, lang: Lang) -> String {
    use cave_home_unifi_access::LockState;
    let state = match (door.lock, lang) {
        (LockState::Locked, Lang::En) => "locked",
        (LockState::Locked, Lang::De) => "verriegelt",
        (LockState::Locked, Lang::Tr) => "kilitli",
        (LockState::Unlocked, Lang::En) => "unlocked",
        (LockState::Unlocked, Lang::De) => "entriegelt",
        (LockState::Unlocked, Lang::Tr) => "açık",
        (LockState::Unknown, Lang::En) => "unknown",
        (LockState::Unknown, Lang::De) => "unbekannt",
        (LockState::Unknown, Lang::Tr) => "bilinmiyor",
    };
    format!("• {} — {state}", door.name)
}

/// Render one camera as a household line.
#[must_use]
pub fn camera_line(camera: &ProtectCamera, lang: Lang) -> String {
    let mut line = format!("• {}", camera.name);
    if camera.is_doorbell {
        let bell = match lang {
            Lang::En => "doorbell",
            Lang::De => "Türklingel",
            Lang::Tr => "kapı zili",
        };
        let _ = write!(line, " ({bell})");
    }
    if !camera.is_online() {
        let off = match lang {
            Lang::En | Lang::De => "offline",
            Lang::Tr => "çevrimdışı",
        };
        let _ = write!(line, " — {off}");
    }
    line
}

/// A renderable list section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Network infrastructure devices.
    Devices,
    /// Network clients.
    Clients,
    /// Access doors.
    Doors,
    /// Protect cameras.
    Cameras,
}

/// Render a header for a list section.
#[must_use]
pub fn header(section: Section, lang: Lang) -> String {
    match (section, lang) {
        (Section::Devices, Lang::En) => "Network devices:",
        (Section::Devices, Lang::De) => "Netzwerkgeräte:",
        (Section::Devices, Lang::Tr) => "Ağ cihazları:",
        (Section::Clients, Lang::En) => "Connected devices:",
        (Section::Clients, Lang::De) => "Verbundene Geräte:",
        (Section::Clients, Lang::Tr) => "Bağlı cihazlar:",
        (Section::Doors, Lang::En) => "Doors:",
        (Section::Doors, Lang::De) => "Türen:",
        (Section::Doors, Lang::Tr) => "Kapılar:",
        (Section::Cameras, Lang::En) => "Cameras:",
        (Section::Cameras, Lang::De) => "Kameras:",
        (Section::Cameras, Lang::Tr) => "Kameralar:",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_unifi_access::{DoorId, DoorPosition, LockState};
    use cave_home_unifi_network::DeviceKind;
    use cave_home_unifi_protect::{CameraId, ProtectCamera};

    #[test]
    fn device_line_renders_kind_and_state() {
        let d = NetworkDevice::new("d1", "Salon switch", "m", DeviceKind::Switch);
        assert_eq!(device_line(&d, Lang::En), "• Salon switch (switch) — online");
        let off = NetworkDevice::new("d2", "Garaj", "m", DeviceKind::AccessPoint).offline();
        assert_eq!(
            device_line(&off, Lang::Tr),
            "• Garaj (Wi-Fi point) — çevrimdışı"
        );
    }

    #[test]
    fn client_line_wireless_and_blocked() {
        let c = NetworkClient::new("aa", "Kid tablet")
            .wireless("Home", "ap1")
            .blocked();
        assert_eq!(client_line(&c, Lang::En), "• Kid tablet — on Home [blocked]");
    }

    #[test]
    fn client_line_wired_tr() {
        let c = NetworkClient::new("aa", "TV");
        assert_eq!(client_line(&c, Lang::Tr), "• TV — kablolu");
    }

    #[test]
    fn door_line_lock_states() {
        let mut d = DoorStatus {
            id: DoorId::new("d1"),
            name: "Front door".into(),
            lock: LockState::Locked,
            position: DoorPosition::Closed,
            online: true,
        };
        assert_eq!(door_line(&d, Lang::En), "• Front door — locked");
        d.lock = LockState::Unlocked;
        assert_eq!(door_line(&d, Lang::Tr), "• Front door — açık");
    }

    #[test]
    fn camera_line_doorbell_offline() {
        let cam = ProtectCamera::new(CameraId::new("c"), "Front", "m")
            .as_doorbell()
            .with_state(cave_home_unifi_protect::DeviceState::Offline);
        assert_eq!(camera_line(&cam, Lang::En), "• Front (doorbell) — offline");
    }

    #[test]
    fn headers_localize() {
        assert_eq!(header(Section::Doors, Lang::Tr), "Kapılar:");
        assert_eq!(header(Section::Cameras, Lang::De), "Kameras:");
    }
}
