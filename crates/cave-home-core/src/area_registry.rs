//! Port of `homeassistant.helpers.area_registry`.
//!
//! The area registry is a flat `id -> AreaEntry` store. Areas are created by
//! name; the id is the slug of that name (de-duplicated with `_2`/`_3`
//! suffixes), and a second create with a name that normalises to an existing
//! area's name is rejected. Areas can carry aliases, an optional floor, an
//! icon and a picture.

use crate::util::{ensure_unique_string, slugify};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AreaError {
    #[error("area name {0:?} is already in use")]
    DuplicateName(String),
    #[error("no area with id {0:?}")]
    UnknownId(String),
    #[error("area name must not be empty")]
    EmptyName,
}

/// Port of `homeassistant.helpers.area_registry.AreaEntry`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
}

/// Normalise a name for duplicate detection. HA's area registry compares
/// names after folding case, whitespace and punctuation — the same fold the
/// slug id uses — so `"Living Room"`, `"living-room"` and `"Living  Room!"`
/// are all the same area name.
#[must_use]
fn normalize_name(name: &str) -> String {
    slugify(name)
}

#[derive(Default)]
struct AreaInner {
    areas: BTreeMap<String, AreaEntry>,
}

/// Port of `homeassistant.helpers.area_registry.AreaRegistry`.
#[derive(Clone, Default)]
pub struct AreaRegistry {
    inner: Arc<RwLock<AreaInner>>,
}

impl AreaRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `async_create` — create an area from a name, generating a unique
    /// slug id. Rejects an empty name or a name colliding (after
    /// normalisation) with an existing area.
    ///
    /// # Errors
    /// [`AreaError::EmptyName`] if `name` slugs to nothing;
    /// [`AreaError::DuplicateName`] if it normalises to an existing area.
    pub fn create(&self, name: impl Into<String>) -> Result<AreaEntry, AreaError> {
        let name = name.into();
        let normalized = normalize_name(&name);
        if normalized.is_empty() {
            return Err(AreaError::EmptyName);
        }
        let mut guard = self.inner.write();
        if guard.areas.values().any(|a| normalize_name(&a.name) == normalized) {
            return Err(AreaError::DuplicateName(name));
        }
        let existing: HashSet<String> = guard.areas.keys().cloned().collect();
        let id = ensure_unique_string(&slugify(&name), &existing);
        let entry = AreaEntry { id: id.clone(), name, ..AreaEntry::default() };
        guard.areas.insert(id, entry.clone());
        Ok(entry)
    }

    /// `async_get_area`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<AreaEntry> {
        self.inner.read().areas.get(id).cloned()
    }

    /// `async_get_area_by_name` — match on the normalised name.
    #[must_use]
    pub fn get_by_name(&self, name: &str) -> Option<AreaEntry> {
        let normalized = normalize_name(name);
        self.inner
            .read()
            .areas
            .values()
            .find(|a| normalize_name(&a.name) == normalized)
            .cloned()
    }

    /// `async_update` — replace the mutable fields of an existing area.
    /// Renaming to a name that collides with a *different* area is rejected.
    ///
    /// # Errors
    /// [`AreaError::UnknownId`] if `id` is not registered;
    /// [`AreaError::EmptyName`] / [`AreaError::DuplicateName`] on a bad rename.
    pub fn update(&self, id: &str, changes: AreaUpdate) -> Result<AreaEntry, AreaError> {
        let mut guard = self.inner.write();
        if !guard.areas.contains_key(id) {
            return Err(AreaError::UnknownId(id.to_owned()));
        }
        // A rename must not collide with a *different* area's normalised name.
        if let Some(new_name) = &changes.name {
            let normalized = normalize_name(new_name);
            if normalized.is_empty() {
                return Err(AreaError::EmptyName);
            }
            if guard
                .areas
                .iter()
                .any(|(other_id, a)| other_id != id && normalize_name(&a.name) == normalized)
            {
                return Err(AreaError::DuplicateName(new_name.clone()));
            }
        }
        let Some(entry) = guard.areas.get_mut(id) else {
            return Err(AreaError::UnknownId(id.to_owned()));
        };
        if let Some(name) = changes.name {
            entry.name = name;
        }
        if let Some(aliases) = changes.aliases {
            entry.aliases = aliases;
        }
        if let Some(floor_id) = changes.floor_id {
            entry.floor_id = floor_id;
        }
        if let Some(icon) = changes.icon {
            entry.icon = icon;
        }
        if let Some(picture) = changes.picture {
            entry.picture = picture;
        }
        Ok(entry.clone())
    }

    /// `async_delete` — remove an area, returning the removed entry.
    #[must_use]
    pub fn delete(&self, id: &str) -> Option<AreaEntry> {
        self.inner.write().areas.remove(id)
    }

    /// `async_list_areas` — every area, ordered by id.
    #[must_use]
    pub fn list(&self) -> Vec<AreaEntry> {
        self.inner.read().areas.values().cloned().collect()
    }
}

/// Field-level changes for [`AreaRegistry::update`]. `None` leaves a field
/// untouched; `Some` overwrites it.
#[derive(Clone, Debug, Default)]
pub struct AreaUpdate {
    pub name: Option<String>,
    pub aliases: Option<BTreeSet<String>>,
    pub floor_id: Option<Option<String>>,
    pub icon: Option<Option<String>>,
    pub picture: Option<Option<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_generates_slug_id_and_dedupes() {
        let reg = AreaRegistry::new();
        let a = reg.create("Living Room").expect("create");
        assert_eq!(a.id, "living_room");
        assert_eq!(a.name, "Living Room");
        // a different name that slugs to the same id gets a _2 suffix
        let b = reg.create("Living  Room!").map(|e| e.id);
        // "Living  Room!" normalises to "living room" — a duplicate name
        assert_eq!(b, Err(AreaError::DuplicateName("Living  Room!".into())));
        // genuinely different name, colliding slug
        let c = reg.create("Living Room 2").expect("c");
        assert_eq!(c.id, "living_room_2");
    }

    #[test]
    fn empty_name_rejected() {
        let reg = AreaRegistry::new();
        assert_eq!(reg.create("   ").unwrap_err(), AreaError::EmptyName);
    }

    #[test]
    fn get_and_get_by_name() {
        let reg = AreaRegistry::new();
        let a = reg.create("Kitchen").expect("create");
        assert_eq!(reg.get(&a.id), Some(a.clone()));
        // name lookup is normalised (case / whitespace insensitive)
        assert_eq!(reg.get_by_name("  kitchen ").map(|e| e.id), Some("kitchen".into()));
        assert!(reg.get_by_name("nowhere").is_none());
    }

    #[test]
    fn update_fields_and_rename_collision() {
        let reg = AreaRegistry::new();
        let kitchen = reg.create("Kitchen").expect("k");
        let bedroom = reg.create("Bedroom").expect("b");

        let changes = AreaUpdate {
            floor_id: Some(Some("ground".into())),
            aliases: Some(BTreeSet::from(["cookspace".to_owned()])),
            ..AreaUpdate::default()
        };
        let updated = reg.update(&kitchen.id, changes).expect("update");
        assert_eq!(updated.floor_id.as_deref(), Some("ground"));
        assert!(updated.aliases.contains("cookspace"));
        // id never changes on update
        assert_eq!(updated.id, "kitchen");

        // renaming bedroom → "Kitchen" collides
        let rename = AreaUpdate { name: Some("Kitchen".into()), ..AreaUpdate::default() };
        assert_eq!(
            reg.update(&bedroom.id, rename).unwrap_err(),
            AreaError::DuplicateName("Kitchen".into())
        );
        // renaming to a fresh name works and frees the old name
        let rename_ok = AreaUpdate { name: Some("Main Bedroom".into()), ..AreaUpdate::default() };
        let r = reg.update(&bedroom.id, rename_ok).expect("rename");
        assert_eq!(r.name, "Main Bedroom");
        assert!(reg.get_by_name("bedroom").is_none());
    }

    #[test]
    fn update_unknown_id_errors() {
        let reg = AreaRegistry::new();
        assert_eq!(
            reg.update("ghost", AreaUpdate::default()).unwrap_err(),
            AreaError::UnknownId("ghost".into())
        );
    }

    #[test]
    fn delete_removes_and_frees_name() {
        let reg = AreaRegistry::new();
        let a = reg.create("Garage").expect("create");
        assert_eq!(reg.delete(&a.id), Some(a));
        assert!(reg.get("garage").is_none());
        // name is free again
        let b = reg.create("Garage").expect("recreate");
        assert_eq!(b.id, "garage");
        assert!(reg.delete("garage").is_some());
        assert!(reg.delete("garage").is_none());
    }

    #[test]
    fn list_is_ordered_by_id() {
        let reg = AreaRegistry::new();
        reg.create("Zen Den").expect("z");
        reg.create("Attic").expect("a");
        let ids: Vec<_> = reg.list().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["attic", "zen_den"]);
    }

    // silence unused-import warnings in the RED skeleton
    #[allow(dead_code)]
    fn _uses(s: &HashSet<String>) -> String {
        ensure_unique_string("x", s) + &slugify("y")
    }
}
