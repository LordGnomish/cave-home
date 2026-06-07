// SPDX-License-Identifier: Apache-2.0
//! The typed Lovelace-class card model.
//!
//! A dashboard is built from [`Card`]s. Each card variant names the resident-
//! facing widget it renders. Some card kinds are **developer-only** (raw entity
//! inspector, cluster topology, logs); the [`crate::dashboard`] layout engine
//! drops those entirely in Resident mode (Charter §6.3).
//!
//! Cards reference entities by their opaque id; the id is plumbing and is never
//! shown to the resident — the friendly name comes from the view-model.

use crate::area::Domain;

/// One card on a dashboard view. First-party, not a copy of any upstream
/// schema: just the widget kinds cave-home's Portal needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Card {
    /// A single device with its status and primary action.
    Entity { entity_id: String },
    /// A compact strip of several devices (the at-a-glance row).
    Glance { entity_ids: Vec<String> },
    /// A big tap-target button (usually a scene or a single toggle).
    Button { entity_id: String },
    /// A thermostat control.
    Thermostat { entity_id: String },
    /// A cover (blind / curtain / garage door) control.
    Cover { entity_id: String },
    /// A light with brightness / colour control.
    Light { entity_id: String },
    /// A camera live view.
    Camera { entity_id: String },
    /// A small history graph for one sensor.
    SensorGraph { entity_id: String },
    /// A one-tap scene activator.
    Scene { entity_id: String },
    /// A summary of a whole room (count of lights on, temperature, …).
    AreaSummary { area_id: String },
    // ---- developer-only cards (hidden from residents, mobile) --------------
    /// Raw device state inspector — id, attributes, last-seen. Developer only.
    RawEntity { entity_id: String },
    /// Cluster / hub topology. Developer only.
    ClusterTopology,
    /// ServiceLB (K3s svclb) status — the LoadBalancer Services exposed on the
    /// cluster, which are published vs pending. Developer only.
    ServiceLb,
    /// Live log tail for an add-on. Developer only.
    Logs { entity_id: String },
}

impl Card {
    /// Whether this card kind is part of the power-user surface and must be
    /// hidden from residents and from the mobile app (Charter §6.3).
    #[must_use]
    pub const fn is_developer_only(&self) -> bool {
        matches!(
            self,
            Self::RawEntity { .. } | Self::ClusterTopology | Self::ServiceLb | Self::Logs { .. }
        )
    }

    /// The default card kind to render a single entity of `domain`. This is the
    /// per-domain choice the auto-dashboard generator makes.
    #[must_use]
    pub fn default_for(domain: Domain, entity_id: impl Into<String>) -> Self {
        let id = entity_id.into();
        match domain {
            Domain::Light => Self::Light { entity_id: id },
            Domain::Cover => Self::Cover { entity_id: id },
            Domain::Climate => Self::Thermostat { entity_id: id },
            Domain::Camera => Self::Camera { entity_id: id },
            Domain::Sensor => Self::SensorGraph { entity_id: id },
            Domain::Scene => Self::Scene { entity_id: id },
            // Switches and media players read well as a single Entity card;
            // a binary sensor (motion/contact) is a plain Entity status tile.
            Domain::Switch | Domain::MediaPlayer | Domain::BinarySensor | Domain::Lock => {
                Self::Entity { entity_id: id }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn developer_cards_flagged() {
        assert!(Card::ClusterTopology.is_developer_only());
        assert!(Card::RawEntity { entity_id: "x".into() }.is_developer_only());
        assert!(Card::Logs { entity_id: "x".into() }.is_developer_only());
    }

    #[test]
    fn resident_cards_not_flagged() {
        assert!(!Card::Entity { entity_id: "x".into() }.is_developer_only());
        assert!(!Card::Light { entity_id: "x".into() }.is_developer_only());
        assert!(!Card::AreaSummary { area_id: "a".into() }.is_developer_only());
        assert!(!Card::Scene { entity_id: "s".into() }.is_developer_only());
    }

    #[test]
    fn default_card_per_domain() {
        assert!(matches!(
            Card::default_for(Domain::Light, "l"),
            Card::Light { .. }
        ));
        assert!(matches!(
            Card::default_for(Domain::Climate, "c"),
            Card::Thermostat { .. }
        ));
        assert!(matches!(
            Card::default_for(Domain::Cover, "v"),
            Card::Cover { .. }
        ));
        assert!(matches!(
            Card::default_for(Domain::Camera, "cam"),
            Card::Camera { .. }
        ));
        assert!(matches!(
            Card::default_for(Domain::Sensor, "s"),
            Card::SensorGraph { .. }
        ));
        assert!(matches!(
            Card::default_for(Domain::Scene, "sc"),
            Card::Scene { .. }
        ));
        assert!(matches!(
            Card::default_for(Domain::Switch, "w"),
            Card::Entity { .. }
        ));
        assert!(matches!(
            Card::default_for(Domain::Lock, "k"),
            Card::Entity { .. }
        ));
    }

    #[test]
    fn every_domain_maps_to_a_resident_card() {
        for d in Domain::ALL {
            let c = Card::default_for(d, "e");
            assert!(!c.is_developer_only(), "{d:?} auto-mapped to a dev card");
        }
    }
}
