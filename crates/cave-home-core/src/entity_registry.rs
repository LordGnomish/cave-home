//! Port of `homeassistant.helpers.entity_registry`.
//!
//! Maps an integration's stable `unique_id` (scoped by `platform` and
//! `domain`) to a user-facing `entity_id` of the form `domain.object_id`.
//! `get_or_create` is idempotent on `(domain, platform, unique_id)`: the first
//! call allocates an `entity_id` (slug of a suggested object id or the unique
//! id, de-duplicated with `_2`/`_3`), later calls return the same entry. Each
//! entry can be disabled, hidden, named over, and linked to a device/area.

use crate::entity::EntityCategory;
use crate::util::{ensure_unique_string, slugify};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EntityRegistryError {
    #[error("no registry entry for entity_id {0:?}")]
    UnknownEntityId(String),
}

/// Why an entry is disabled (`homeassistant.helpers.entity_registry.RegistryEntryDisabler`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisabledBy {
    User,
    Integration,
    Config,
    Device,
}

/// Why an entry is hidden (`homeassistant.helpers.entity_registry.RegistryEntryHider`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HiddenBy {
    User,
    Integration,
}

/// Port of `homeassistant.helpers.entity_registry.RegistryEntry`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub entity_id: String,
    pub unique_id: String,
    pub platform: String,
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_by: Option<DisabledBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_by: Option<HiddenBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_category: Option<EntityCategory>,
    /// User-supplied name override (the registry's `name`, distinct from the
    /// entity's own `original_name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl RegistryEntry {
    /// `RegistryEntry.disabled` — true when any disabler is set.
    #[must_use]
    pub fn disabled(&self) -> bool {
        self.disabled_by.is_some()
    }

    /// `RegistryEntry.hidden` — true when any hider is set.
    #[must_use]
    pub fn hidden(&self) -> bool {
        self.hidden_by.is_some()
    }
}

/// Parameters for [`EntityRegistry::get_or_create`] beyond the identity triple.
#[derive(Clone, Debug, Default)]
pub struct EntityCreate {
    pub suggested_object_id: Option<String>,
    pub device_id: Option<String>,
    pub entity_category: Option<EntityCategory>,
    pub disabled_by: Option<DisabledBy>,
}

#[derive(Default)]
struct EntityRegInner {
    /// entity_id -> entry
    entries: HashMap<String, RegistryEntry>,
    /// (platform, domain, unique_id) -> entity_id
    by_unique: HashMap<(String, String, String), String>,
}

/// Port of `homeassistant.helpers.entity_registry.EntityRegistry`.
#[derive(Clone, Default)]
pub struct EntityRegistry {
    inner: Arc<RwLock<EntityRegInner>>,
}

impl EntityRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `async_get_or_create`. Idempotent on `(domain, platform, unique_id)`:
    /// allocates a fresh `entity_id` on first sight, returns the existing entry
    /// thereafter (ignoring `opts` on the repeat call, as upstream does).
    pub fn get_or_create(
        &self,
        domain: &str,
        platform: &str,
        unique_id: &str,
        opts: EntityCreate,
    ) -> RegistryEntry {
        let _ = (domain, platform, unique_id, opts);
        unimplemented!("RED")
    }

    /// `async_get` — fetch by `entity_id`.
    #[must_use]
    pub fn get(&self, entity_id: &str) -> Option<RegistryEntry> {
        self.inner.read().entries.get(entity_id).cloned()
    }

    /// `async_get_entity_id` — resolve identity triple to an `entity_id`.
    #[must_use]
    pub fn get_entity_id(&self, domain: &str, platform: &str, unique_id: &str) -> Option<String> {
        self.inner
            .read()
            .by_unique
            .get(&(platform.to_owned(), domain.to_owned(), unique_id.to_owned()))
            .cloned()
    }

    /// `async_update_entity` — overwrite mutable fields.
    pub fn update(
        &self,
        entity_id: &str,
        changes: EntityUpdate,
    ) -> Result<RegistryEntry, EntityRegistryError> {
        let _ = (entity_id, changes);
        unimplemented!("RED")
    }

    /// `async_remove` — drop an entry and its identity index.
    pub fn remove(&self, entity_id: &str) -> Option<RegistryEntry> {
        let mut guard = self.inner.write();
        let removed = guard.entries.remove(entity_id)?;
        guard.by_unique.remove(&(
            removed.platform.clone(),
            removed.domain.clone(),
            removed.unique_id.clone(),
        ));
        Some(removed)
    }

    /// Every entry linked to `device_id`.
    #[must_use]
    pub fn entities_for_device(&self, device_id: &str) -> Vec<RegistryEntry> {
        let mut v: Vec<_> = self
            .inner
            .read()
            .entries
            .values()
            .filter(|e| e.device_id.as_deref() == Some(device_id))
            .cloned()
            .collect();
        v.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
        v
    }

    /// Every entry, ordered by `entity_id`.
    #[must_use]
    pub fn list(&self) -> Vec<RegistryEntry> {
        let mut v: Vec<_> = self.inner.read().entries.values().cloned().collect();
        v.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
        v
    }
}

/// Field-level changes for [`EntityRegistry::update`].
#[derive(Clone, Debug, Default)]
pub struct EntityUpdate {
    pub name: Option<Option<String>>,
    pub icon: Option<Option<String>>,
    pub area_id: Option<Option<String>>,
    pub disabled_by: Option<Option<DisabledBy>>,
    pub hidden_by: Option<Option<HiddenBy>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_or_create_allocates_entity_id_from_unique_id() {
        let reg = EntityRegistry::new();
        let e = reg.get_or_create("light", "hue", "0017abcd", EntityCreate::default());
        assert_eq!(e.entity_id, "light.0017abcd");
        assert_eq!(e.domain, "light");
        assert_eq!(e.platform, "hue");
        assert!(!e.disabled());
        assert!(!e.hidden());
    }

    #[test]
    fn get_or_create_is_idempotent_on_identity_triple() {
        let reg = EntityRegistry::new();
        let first = reg.get_or_create(
            "light",
            "hue",
            "u1",
            EntityCreate { suggested_object_id: Some("Kitchen Lamp".into()), ..EntityCreate::default() },
        );
        assert_eq!(first.entity_id, "light.kitchen_lamp");
        // a second call with the same triple returns the SAME entity_id
        let again = reg.get_or_create("light", "hue", "u1", EntityCreate::default());
        assert_eq!(again.entity_id, "light.kitchen_lamp");
        assert_eq!(reg.list().len(), 1);
    }

    #[test]
    fn colliding_object_ids_get_numeric_suffix() {
        let reg = EntityRegistry::new();
        let a = reg.get_or_create(
            "light",
            "hue",
            "u1",
            EntityCreate { suggested_object_id: Some("lamp".into()), ..EntityCreate::default() },
        );
        let b = reg.get_or_create(
            "light",
            "tplink",
            "u2",
            EntityCreate { suggested_object_id: Some("lamp".into()), ..EntityCreate::default() },
        );
        assert_eq!(a.entity_id, "light.lamp");
        assert_eq!(b.entity_id, "light.lamp_2");
        // a different domain with the same object id does NOT collide
        let c = reg.get_or_create(
            "switch",
            "hue",
            "u3",
            EntityCreate { suggested_object_id: Some("lamp".into()), ..EntityCreate::default() },
        );
        assert_eq!(c.entity_id, "switch.lamp");
    }

    #[test]
    fn get_entity_id_and_device_link() {
        let reg = EntityRegistry::new();
        let e = reg.get_or_create(
            "sensor",
            "zwave",
            "node5-temp",
            EntityCreate { device_id: Some("dev1".into()), ..EntityCreate::default() },
        );
        assert_eq!(reg.get_entity_id("sensor", "zwave", "node5-temp"), Some(e.entity_id.clone()));
        assert!(reg.get_entity_id("sensor", "zwave", "nope").is_none());
        assert_eq!(reg.entities_for_device("dev1").len(), 1);
        assert!(reg.entities_for_device("other").is_empty());
    }

    #[test]
    fn update_disable_hide_and_rename() {
        let reg = EntityRegistry::new();
        let e = reg.get_or_create("light", "hue", "u1", EntityCreate::default());
        let u = reg
            .update(
                &e.entity_id,
                EntityUpdate {
                    name: Some(Some("Reading light".into())),
                    disabled_by: Some(Some(DisabledBy::User)),
                    hidden_by: Some(Some(HiddenBy::Integration)),
                    ..EntityUpdate::default()
                },
            )
            .expect("update");
        assert_eq!(u.name.as_deref(), Some("Reading light"));
        assert!(u.disabled());
        assert_eq!(u.disabled_by, Some(DisabledBy::User));
        assert!(u.hidden());

        // clearing a disabler with Some(None)
        let cleared = reg
            .update(&e.entity_id, EntityUpdate { disabled_by: Some(None), ..EntityUpdate::default() })
            .expect("clear");
        assert!(!cleared.disabled());
        // name untouched by this update (None leaves it)
        assert_eq!(cleared.name.as_deref(), Some("Reading light"));

        assert_eq!(
            reg.update("light.ghost", EntityUpdate::default()).unwrap_err(),
            EntityRegistryError::UnknownEntityId("light.ghost".into())
        );
    }

    #[test]
    fn remove_frees_identity_and_object_id() {
        let reg = EntityRegistry::new();
        let e = reg.get_or_create(
            "light",
            "hue",
            "u1",
            EntityCreate { suggested_object_id: Some("lamp".into()), ..EntityCreate::default() },
        );
        assert!(reg.remove(&e.entity_id).is_some());
        assert!(reg.get(&e.entity_id).is_none());
        assert!(reg.get_entity_id("light", "hue", "u1").is_none());
        // object id is free again → no suffix
        let again = reg.get_or_create(
            "light",
            "hue",
            "u1",
            EntityCreate { suggested_object_id: Some("lamp".into()), ..EntityCreate::default() },
        );
        assert_eq!(again.entity_id, "light.lamp");
    }

    #[allow(dead_code)]
    fn _uses() {
        let _ = ensure_unique_string("x", &std::collections::HashSet::new());
        let _ = slugify("y");
    }
}
