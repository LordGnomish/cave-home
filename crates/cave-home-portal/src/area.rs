// SPDX-License-Identifier: Apache-2.0
//! The grandma-friendly navigation tree: Home → Areas (rooms) → Entities.
//!
//! "Rooms over hierarchies" (`docs/ui-language.md`): a resident navigates by
//! room, never by technology. An [`Area`] is a room ("Living room"); an
//! [`Entity`] is one controllable thing in it (a light, a lock, a sensor).
//!
//! This module is pure data + lookup; it holds no network or protocol state.

/// The device classes the Portal knows how to render. This is the Lovelace-class
/// "domain" set, named in plain terms. New domains are added here as the rest of
/// cave-home grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Domain {
    /// A light or lamp.
    Light,
    /// A switch / smart plug.
    Switch,
    /// A door lock.
    Lock,
    /// A blind, curtain, garage door — anything that opens and closes.
    Cover,
    /// A thermostat / heating zone.
    Climate,
    /// A camera feed.
    Camera,
    /// A read-only measurement (temperature, humidity, power, …).
    Sensor,
    /// A binary read-only sensor (motion, door-open, …).
    BinarySensor,
    /// A saved scene ("Evening", "Movie night").
    Scene,
    /// A media player (speaker / TV).
    MediaPlayer,
}

impl Domain {
    /// All domains, for exhaustive iteration in tests and auto-generation.
    pub const ALL: [Self; 10] = [
        Self::Light,
        Self::Switch,
        Self::Lock,
        Self::Cover,
        Self::Climate,
        Self::Camera,
        Self::Sensor,
        Self::BinarySensor,
        Self::Scene,
        Self::MediaPlayer,
    ];

    /// Whether a domain is something the resident *controls* (vs. only reads).
    /// Used by auto-generation to decide button affordances.
    #[must_use]
    pub const fn is_controllable(self) -> bool {
        matches!(
            self,
            Self::Light
                | Self::Switch
                | Self::Lock
                | Self::Cover
                | Self::Climate
                | Self::Scene
                | Self::MediaPlayer
        )
    }
}

/// One controllable / observable thing in the home. `id` is an opaque stable
/// handle (never shown to the resident); `name` is the friendly name and `area`
/// the room it lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    /// Opaque stable identity. Developer-view only; never rendered on a tile.
    pub id: String,
    /// Friendly, resident-facing name ("Ceiling light").
    pub name: String,
    /// What kind of device this is.
    pub domain: Domain,
    /// The id of the [`Area`] this entity belongs to, if assigned.
    pub area: Option<String>,
}

impl Entity {
    /// Build an entity. `id` and `name` are taken verbatim; trimming/validation
    /// of friendly names is the caller's job (the device-onboarding flow).
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        domain: Domain,
        area: Option<impl Into<String>>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            domain,
            area: area.map(Into::into),
        }
    }
}

/// A room. `icon` is a logical icon name the (deferred) frontend maps to a glyph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    /// Opaque stable id.
    pub id: String,
    /// Friendly room name ("Living room").
    pub name: String,
    /// Logical icon name ("sofa", "bed", "kitchen", …).
    pub icon: String,
}

impl Area {
    /// Build a room.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, icon: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            icon: icon.into(),
        }
    }
}

/// The whole home: its rooms and its entities. This is the registry the Portal
/// reads to build a navigation tree and to auto-generate a dashboard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Home {
    areas: Vec<Area>,
    entities: Vec<Entity>,
}

impl Home {
    /// An empty home.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a room. If a room with the same id already exists it is replaced
    /// (idempotent re-sync from the device registry).
    pub fn add_area(&mut self, area: Area) -> &mut Self {
        if let Some(slot) = self.areas.iter_mut().find(|a| a.id == area.id) {
            *slot = area;
        } else {
            self.areas.push(area);
        }
        self
    }

    /// Register an entity (same replace-by-id semantics as [`Home::add_area`]).
    pub fn add_entity(&mut self, entity: Entity) -> &mut Self {
        if let Some(slot) = self.entities.iter_mut().find(|e| e.id == entity.id) {
            *slot = entity;
        } else {
            self.entities.push(entity);
        }
        self
    }

    /// All rooms, in insertion order.
    #[must_use]
    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    /// All entities, in insertion order.
    #[must_use]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// Look up a room by id.
    #[must_use]
    pub fn area(&self, id: &str) -> Option<&Area> {
        self.areas.iter().find(|a| a.id == id)
    }

    /// The entities assigned to a given room, in insertion order.
    #[must_use]
    pub fn entities_in(&self, area_id: &str) -> Vec<&Entity> {
        self.entities
            .iter()
            .filter(|e| e.area.as_deref() == Some(area_id))
            .collect()
    }

    /// Entities not assigned to any room. The Portal groups these under an
    /// "Other" view so nothing is ever silently hidden.
    #[must_use]
    pub fn unassigned(&self) -> Vec<&Entity> {
        self.entities.iter().filter(|e| e.area.is_none()).collect()
    }

    /// `true` when the home has no rooms and no devices (drives the empty state).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.areas.is_empty() && self.entities.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Home {
        let mut h = Home::new();
        h.add_area(Area::new("living", "Living room", "sofa"));
        h.add_area(Area::new("bed", "Bedroom", "bed"));
        h.add_entity(Entity::new(
            "l1",
            "Ceiling light",
            Domain::Light,
            Some("living"),
        ));
        h.add_entity(Entity::new("l2", "Floor lamp", Domain::Light, Some("living")));
        h.add_entity(Entity::new("lk", "Front door", Domain::Lock, Some("living")));
        h.add_entity(Entity::new(
            "bl",
            "Reading light",
            Domain::Light,
            Some("bed"),
        ));
        h.add_entity(Entity::new(
            "orphan",
            "Garage plug",
            Domain::Switch,
            None::<String>,
        ));
        h
    }

    #[test]
    fn empty_home_reports_empty() {
        assert!(Home::new().is_empty());
        assert!(!sample().is_empty());
    }

    #[test]
    fn entities_group_by_area() {
        let h = sample();
        assert_eq!(h.entities_in("living").len(), 3);
        assert_eq!(h.entities_in("bed").len(), 1);
        assert_eq!(h.entities_in("nope").len(), 0);
    }

    #[test]
    fn unassigned_entities_are_surfaced() {
        let h = sample();
        let orphans = h.unassigned();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].name, "Garage plug");
    }

    #[test]
    fn area_lookup_works() {
        let h = sample();
        assert_eq!(h.area("bed").map(|a| a.name.as_str()), Some("Bedroom"));
        assert!(h.area("missing").is_none());
    }

    #[test]
    fn add_is_idempotent_by_id() {
        let mut h = Home::new();
        h.add_area(Area::new("living", "Living room", "sofa"));
        h.add_area(Area::new("living", "Lounge", "sofa")); // re-sync rename
        assert_eq!(h.areas().len(), 1);
        assert_eq!(h.area("living").map(|a| a.name.as_str()), Some("Lounge"));

        h.add_entity(Entity::new("x", "A", Domain::Light, Some("living")));
        h.add_entity(Entity::new("x", "B", Domain::Light, Some("living")));
        assert_eq!(h.entities().len(), 1);
        assert_eq!(h.entities()[0].name, "B");
    }

    #[test]
    fn controllable_domains_classified() {
        assert!(Domain::Light.is_controllable());
        assert!(Domain::Lock.is_controllable());
        assert!(!Domain::Sensor.is_controllable());
        assert!(!Domain::Camera.is_controllable());
        assert!(!Domain::BinarySensor.is_controllable());
    }
}
