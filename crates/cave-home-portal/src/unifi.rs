// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Portal `/unifi` page view-model.
//!
//! A household overview of the UniFi stack.
//!
//! It shows how many network devices and clients are up, how many cameras are
//! online, and how many doors are locked — plus the latest real-time line
//! ("Doorbell at the front door").
//!
//! Like the rest of `cave-home-portal` this is a **pure UI model** — std-only,
//! no network, no dependency on the device adapters. The `cave-home-unifi`
//! client feeds it plain counts and strings; this module turns them into a
//! grandma-friendly, localised page (Charter §6.3, ADR-007).

// "UniFi" reads as a code identifier to clippy's doc-markdown lint; this is a
// product name in prose, as in the sibling cave-home-unifi-* crates.
#![allow(clippy::doc_markdown)]

use crate::label::Lang;

/// A single status tile on the overview page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusTile {
    /// The localised tile title.
    pub title: String,
    /// The value shown big (e.g. "8 / 9").
    pub value: String,
    /// Whether everything in this tile is healthy (drives the colour).
    pub healthy: bool,
}

/// A live counts snapshot fed in by the `cave-home-unifi` client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnifiCounts {
    /// Network infrastructure devices online.
    pub devices_online: u32,
    /// Network infrastructure devices total.
    pub devices_total: u32,
    /// Connected clients.
    pub clients: u32,
    /// Cameras online.
    pub cameras_online: u32,
    /// Cameras total.
    pub cameras_total: u32,
    /// Doors locked.
    pub doors_locked: u32,
    /// Doors total.
    pub doors_total: u32,
}

/// The `/unifi` overview page view-model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiOverviewPage {
    /// The counts snapshot.
    pub counts: UnifiCounts,
    /// The latest real-time line (already localised by the caller), if any.
    pub latest_event: Option<String>,
}

impl UnifiOverviewPage {
    /// Build a page from a counts snapshot.
    #[must_use]
    pub const fn new(counts: UnifiCounts) -> Self {
        Self {
            counts,
            latest_event: None,
        }
    }

    /// Builder: attach the latest event line.
    #[must_use]
    pub fn with_latest_event(mut self, line: impl Into<String>) -> Self {
        self.latest_event = Some(line.into());
        self
    }

    /// The localised page title.
    #[must_use]
    pub const fn title(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => "Network & Doors",
            Lang::De => "Netzwerk & Türen",
            Lang::Tr => "Ağ ve Kapılar",
        }
    }

    /// The four status tiles, in display order.
    #[must_use]
    pub fn tiles(&self, lang: Lang) -> Vec<StatusTile> {
        let c = &self.counts;
        let devices_title = match lang {
            Lang::En => "Devices",
            Lang::De => "Geräte",
            Lang::Tr => "Cihazlar",
        };
        let clients_title = match lang {
            Lang::En => "Connected",
            Lang::De => "Verbunden",
            Lang::Tr => "Bağlı",
        };
        let cameras_title = match lang {
            Lang::En => "Cameras",
            Lang::De => "Kameras",
            Lang::Tr => "Kameralar",
        };
        let doors_title = match lang {
            Lang::En => "Doors locked",
            Lang::De => "Türen verriegelt",
            Lang::Tr => "Kilitli kapılar",
        };
        vec![
            StatusTile {
                title: devices_title.to_string(),
                value: format!("{} / {}", c.devices_online, c.devices_total),
                healthy: c.devices_online == c.devices_total,
            },
            StatusTile {
                title: clients_title.to_string(),
                value: c.clients.to_string(),
                healthy: true,
            },
            StatusTile {
                title: cameras_title.to_string(),
                value: format!("{} / {}", c.cameras_online, c.cameras_total),
                healthy: c.cameras_online == c.cameras_total,
            },
            StatusTile {
                title: doors_title.to_string(),
                value: format!("{} / {}", c.doors_locked, c.doors_total),
                // A door left unlocked is worth flagging.
                healthy: c.doors_locked == c.doors_total,
            },
        ]
    }

    /// Whether everything on the page is healthy (all devices+cameras up, all
    /// doors locked).
    #[must_use]
    pub fn all_healthy(&self, lang: Lang) -> bool {
        self.tiles(lang).iter().all(|t| t.healthy)
    }

    /// A one-line household summary of the whole page.
    #[must_use]
    pub fn summary(&self, lang: Lang) -> String {
        let c = &self.counts;
        match lang {
            Lang::En => format!(
                "{} things connected, {} cameras up, {} of {} doors locked",
                c.clients, c.cameras_online, c.doors_locked, c.doors_total
            ),
            Lang::De => format!(
                "{} Geräte verbunden, {} Kameras aktiv, {} von {} Türen verriegelt",
                c.clients, c.cameras_online, c.doors_locked, c.doors_total
            ),
            Lang::Tr => format!(
                "{} cihaz bağlı, {} kamera aktif, {} / {} kapı kilitli",
                c.clients, c.cameras_online, c.doors_locked, c.doors_total
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> UnifiCounts {
        UnifiCounts {
            devices_online: 9,
            devices_total: 9,
            clients: 12,
            cameras_online: 3,
            cameras_total: 3,
            doors_locked: 2,
            doors_total: 2,
        }
    }

    #[test]
    fn title_localizes() {
        let page = UnifiOverviewPage::new(sample());
        assert_eq!(page.title(Lang::Tr), "Ağ ve Kapılar");
        assert_eq!(page.title(Lang::En), "Network & Doors");
    }

    #[test]
    fn tiles_render_counts_and_health() {
        let page = UnifiOverviewPage::new(sample());
        let tiles = page.tiles(Lang::En);
        assert_eq!(tiles.len(), 4);
        assert_eq!(tiles[0].value, "9 / 9");
        assert!(tiles[0].healthy);
        assert_eq!(tiles[1].value, "12");
        assert!(page.all_healthy(Lang::En));
    }

    #[test]
    fn unlocked_door_makes_doors_tile_unhealthy() {
        let mut counts = sample();
        counts.doors_locked = 1; // one door open
        let page = UnifiOverviewPage::new(counts);
        let tiles = page.tiles(Lang::En);
        let doors = tiles.last().unwrap();
        assert_eq!(doors.value, "1 / 2");
        assert!(!doors.healthy);
        assert!(!page.all_healthy(Lang::En));
    }

    #[test]
    fn offline_device_makes_devices_tile_unhealthy() {
        let mut counts = sample();
        counts.devices_online = 8; // one down
        let page = UnifiOverviewPage::new(counts);
        assert!(!page.tiles(Lang::En)[0].healthy);
    }

    #[test]
    fn summary_is_localized_household_line() {
        let page = UnifiOverviewPage::new(sample());
        assert_eq!(
            page.summary(Lang::Tr),
            "12 cihaz bağlı, 3 kamera aktif, 2 / 2 kapı kilitli"
        );
        assert!(page.summary(Lang::En).contains("12 things connected"));
    }

    #[test]
    fn latest_event_is_optional() {
        let page = UnifiOverviewPage::new(sample());
        assert!(page.latest_event.is_none());
        let page = page.with_latest_event("Doorbell at the front door");
        assert_eq!(page.latest_event.as_deref(), Some("Doorbell at the front door"));
    }
}
