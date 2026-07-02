// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Portal `/hue` ("Lights") page view-model.
//!
//! Models the lighting dashboard: a bridge health line, a row per light (on /
//! brightness), and the scene chips. Pure UI model — std-only, no network and
//! no dependency on the `cave-home-hue` transport. The page emits the existing
//! [`Card::Light`] / [`Card::Scene`] widgets, so it slots straight into the
//! dashboard layout engine. Mirrors the shape of [`crate::energy::EnergyPage`].

use crate::card::Card;
use crate::label::Lang;

/// A Hue bridge's at-a-glance health line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HueBridgeStatus {
    /// Friendly bridge name.
    pub name: String,
    /// Whether the bridge is currently reachable.
    pub reachable: bool,
    /// Lights currently switched on.
    pub lights_on: usize,
    /// Lights known to the bridge.
    pub lights_total: usize,
}

/// One light's row on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HueLightRow {
    /// Opaque entity id (plumbing; never shown to the resident).
    pub entity_id: String,
    /// Friendly light name.
    pub name: String,
    /// On/off.
    pub on: bool,
    /// Brightness as a percentage, 0..=100.
    pub brightness_pct: u8,
}

/// One scene chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HueSceneChip {
    /// Opaque scene entity id.
    pub entity_id: String,
    /// Friendly scene name.
    pub name: String,
}

/// The whole `/hue` (Lights) page view-model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HuePage {
    /// The localised page title.
    pub title: String,
    /// The bridge health line.
    pub bridge: HueBridgeStatus,
    /// One row per light.
    pub lights: Vec<HueLightRow>,
    /// The scene chips.
    pub scenes: Vec<HueSceneChip>,
}

impl HuePage {
    /// The localised page title.
    #[must_use]
    const fn title_for(lang: Lang) -> &'static str {
        match lang {
            Lang::En => "Lights",
            Lang::De => "Lichter",
            Lang::Tr => "Işıklar",
        }
    }

    /// A demo page (shown until the bridge transport is wired into the binary).
    #[must_use]
    pub fn demo(lang: Lang) -> Self {
        let lights = vec![
            HueLightRow {
                entity_id: "hue:light:1".into(),
                name: "Living room".into(),
                on: true,
                brightness_pct: 80,
            },
            HueLightRow {
                entity_id: "hue:light:2".into(),
                name: "Kitchen".into(),
                on: true,
                brightness_pct: 100,
            },
            HueLightRow {
                entity_id: "hue:light:3".into(),
                name: "Bedroom".into(),
                on: false,
                brightness_pct: 0,
            },
        ];
        let lights_on = lights.iter().filter(|l| l.on).count();
        Self {
            title: Self::title_for(lang).to_string(),
            bridge: HueBridgeStatus {
                name: "Hue Bridge".into(),
                reachable: true,
                lights_on,
                lights_total: lights.len(),
            },
            lights,
            scenes: vec![
                HueSceneChip { entity_id: "hue:scene:1".into(), name: "Evening".into() },
                HueSceneChip { entity_id: "hue:scene:2".into(), name: "Reading".into() },
                HueSceneChip { entity_id: "hue:scene:3".into(), name: "Relax".into() },
            ],
        }
    }

    /// How many lights are currently on.
    #[must_use]
    pub fn lights_on(&self) -> usize {
        self.lights.iter().filter(|l| l.on).count()
    }

    /// Build the dashboard cards for this page: a [`Card::Light`] per light then
    /// a [`Card::Scene`] per scene, in display order. None are developer-only,
    /// so the whole page is resident-visible.
    #[must_use]
    pub fn cards(&self) -> Vec<Card> {
        self.lights
            .iter()
            .map(|l| Card::Light { entity_id: l.entity_id.clone() })
            .chain(
                self.scenes
                    .iter()
                    .map(|s| Card::Scene { entity_id: s.entity_id.clone() }),
            )
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_page_is_localised() {
        assert_eq!(HuePage::demo(Lang::En).title, "Lights");
        assert_eq!(HuePage::demo(Lang::De).title, "Lichter");
        assert_eq!(HuePage::demo(Lang::Tr).title, "Işıklar");
    }

    #[test]
    fn demo_page_assembles_bridge_and_lights() {
        let page = HuePage::demo(Lang::En);
        assert!(page.bridge.reachable);
        assert_eq!(page.bridge.lights_total, page.lights.len());
        assert_eq!(page.bridge.lights_on, page.lights_on());
        assert_eq!(page.lights_on(), 2); // living room + kitchen
        assert!(!page.scenes.is_empty());
    }

    #[test]
    fn cards_emit_one_light_then_scene_per_entry() {
        let page = HuePage::demo(Lang::En);
        let cards = page.cards();
        assert_eq!(cards.len(), page.lights.len() + page.scenes.len());
        // first cards are lights, in order
        assert_eq!(
            cards[0],
            Card::Light { entity_id: "hue:light:1".into() }
        );
        // a scene card appears after the lights
        assert!(cards
            .iter()
            .any(|c| *c == Card::Scene { entity_id: "hue:scene:1".into() }));
    }

    #[test]
    fn the_whole_page_is_resident_visible() {
        let page = HuePage::demo(Lang::Tr);
        assert!(page.cards().iter().all(|c| !c.is_developer_only()));
    }
}
