//! Port of `homeassistant.helpers.device_registry`.
//!
//! Physical devices keyed by an opaque generated id. A device is matched on
//! the *set* of its `identifiers` (integration `(domain, id)` tuples) and
//! `connections` (`(type, value)` hardware addresses): `get_or_create` finds
//! any existing device sharing at least one identifier or connection and
//! merges into it, otherwise it mints a new entry. Devices link to a parent
//! hub via `via_device_id`, to an area via `area_id`, and remember which
//! config entries reference them.

use crate::entity::DeviceInfo;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeviceError {
    #[error("device hint has neither identifiers nor connections")]
    NoIdentity,
    #[error("no device with id {0:?}")]
    UnknownId(String),
}

/// Port of `homeassistant.helpers.device_registry.DeviceEntry`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub id: String,
    #[serde(default)]
    pub identifiers: BTreeSet<(String, String)>,
    #[serde(default)]
    pub connections: BTreeSet<(String, String)>,
    #[serde(default)]
    pub config_entries: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// User override for `name` (takes precedence in the frontend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_by_user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sw_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hw_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_id: Option<String>,
}

impl DeviceEntry {
    /// The name shown in the UI — `name_by_user` if the user set one, else the
    /// integration-supplied `name`.
    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.name_by_user.as_deref().or(self.name.as_deref())
    }
}

#[derive(Default)]
struct DeviceInner {
    devices: HashMap<String, DeviceEntry>,
}

/// Port of `homeassistant.helpers.device_registry.DeviceRegistry`.
#[derive(Clone, Default)]
pub struct DeviceRegistry {
    inner: Arc<RwLock<DeviceInner>>,
}

impl DeviceRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `async_get_or_create` — find a device sharing any identifier or
    /// connection with `info` and merge the new metadata into it, or create a
    /// new device. `config_entry_id` is recorded on the (possibly pre-existing)
    /// device. `via_device` in `info` is resolved to the parent's `id` when the
    /// parent is already registered.
    ///
    /// Errors if `info` carries neither identifiers nor connections (HA
    /// rejects identity-less devices).
    pub fn get_or_create(
        &self,
        config_entry_id: &str,
        info: &DeviceInfo,
    ) -> Result<DeviceEntry, DeviceError> {
        let _ = (config_entry_id, info);
        unimplemented!("RED")
    }

    /// `async_get`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<DeviceEntry> {
        self.inner.read().devices.get(id).cloned()
    }

    /// `async_get_device` — locate by a shared identifier or connection.
    #[must_use]
    pub fn get_device(
        &self,
        identifiers: &BTreeSet<(String, String)>,
        connections: &BTreeSet<(String, String)>,
    ) -> Option<DeviceEntry> {
        let _ = (identifiers, connections);
        unimplemented!("RED")
    }

    /// `async_update_device` — overwrite mutable fields.
    pub fn update(&self, id: &str, changes: DeviceUpdate) -> Result<DeviceEntry, DeviceError> {
        let _ = (id, changes);
        unimplemented!("RED")
    }

    /// `async_remove_device`.
    pub fn remove(&self, id: &str) -> Option<DeviceEntry> {
        self.inner.write().devices.remove(id)
    }

    /// Every device assigned to `area_id`.
    #[must_use]
    pub fn devices_for_area(&self, area_id: &str) -> Vec<DeviceEntry> {
        self.inner
            .read()
            .devices
            .values()
            .filter(|d| d.area_id.as_deref() == Some(area_id))
            .cloned()
            .collect()
    }

    /// Every device, ordered by id.
    #[must_use]
    pub fn list(&self) -> Vec<DeviceEntry> {
        let mut v: Vec<_> = self.inner.read().devices.values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }
}

/// Field-level changes for [`DeviceRegistry::update`].
#[derive(Clone, Debug, Default)]
pub struct DeviceUpdate {
    pub area_id: Option<Option<String>>,
    pub name_by_user: Option<Option<String>>,
    pub sw_version: Option<Option<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(ids: &[(&str, &str)]) -> DeviceInfo {
        DeviceInfo {
            identifiers: ids.iter().map(|(d, i)| ((*d).to_owned(), (*i).to_owned())).collect(),
            ..DeviceInfo::default()
        }
    }

    #[test]
    fn get_or_create_creates_then_merges_on_shared_identifier() {
        let reg = DeviceRegistry::new();
        let mut first = info(&[("hue", "0017")]);
        first.manufacturer = Some("Signify".into());
        let a = reg.get_or_create("entry1", &first).expect("create");
        assert!(!a.id.is_empty());
        assert_eq!(a.manufacturer.as_deref(), Some("Signify"));
        assert!(a.config_entries.contains("entry1"));

        // a second call sharing the identifier merges (same id) and fills model
        let mut second = info(&[("hue", "0017")]);
        second.model = Some("LCT015".into());
        let b = reg.get_or_create("entry2", &second).expect("merge");
        assert_eq!(b.id, a.id);
        assert_eq!(b.model.as_deref(), Some("LCT015"));
        // earlier manufacturer is preserved through the merge
        assert_eq!(b.manufacturer.as_deref(), Some("Signify"));
        // both config entries are now recorded
        assert!(b.config_entries.contains("entry1"));
        assert!(b.config_entries.contains("entry2"));
        // only one device exists
        assert_eq!(reg.list().len(), 1);
    }

    #[test]
    fn distinct_identity_creates_distinct_devices() {
        let reg = DeviceRegistry::new();
        let a = reg.get_or_create("e", &info(&[("hue", "a")])).expect("a");
        let b = reg.get_or_create("e", &info(&[("hue", "b")])).expect("b");
        assert_ne!(a.id, b.id);
        assert_eq!(reg.list().len(), 2);
    }

    #[test]
    fn identity_less_hint_is_rejected() {
        let reg = DeviceRegistry::new();
        assert_eq!(
            reg.get_or_create("e", &DeviceInfo::default()).unwrap_err(),
            DeviceError::NoIdentity
        );
    }

    #[test]
    fn via_device_resolves_to_parent_id() {
        let reg = DeviceRegistry::new();
        let bridge = reg.get_or_create("e", &info(&[("hue", "bridge")])).expect("bridge");
        let mut bulb = info(&[("hue", "bulb1")]);
        bulb.via_device = Some(("hue".into(), "bridge".into()));
        let child = reg.get_or_create("e", &bulb).expect("child");
        assert_eq!(child.via_device_id.as_deref(), Some(bridge.id.as_str()));
    }

    #[test]
    fn get_device_matches_on_connection() {
        let reg = DeviceRegistry::new();
        let mut di = info(&[("zwave", "node5")]);
        di.connections = vec![("mac".into(), "aa:bb".into())];
        let d = reg.get_or_create("e", &di).expect("d");
        let conn = BTreeSet::from([("mac".to_owned(), "aa:bb".to_owned())]);
        assert_eq!(
            reg.get_device(&BTreeSet::new(), &conn).map(|x| x.id),
            Some(d.id.clone())
        );
        // unknown identity → None
        assert!(reg.get_device(&BTreeSet::new(), &BTreeSet::new()).is_none());
    }

    #[test]
    fn update_area_and_name_by_user_and_display_name() {
        let reg = DeviceRegistry::new();
        let mut di = info(&[("hue", "x")]);
        di.name = Some("Hue lamp 1".into());
        let d = reg.get_or_create("e", &di).expect("d");
        assert_eq!(d.display_name(), Some("Hue lamp 1"));

        let changes = DeviceUpdate {
            area_id: Some(Some("living_room".into())),
            name_by_user: Some(Some("Reading lamp".into())),
            ..DeviceUpdate::default()
        };
        let u = reg.update(&d.id, changes).expect("update");
        assert_eq!(u.area_id.as_deref(), Some("living_room"));
        // user name now wins
        assert_eq!(u.display_name(), Some("Reading lamp"));
        assert_eq!(reg.devices_for_area("living_room").len(), 1);

        // unknown id errors
        assert_eq!(
            reg.update("ghost", DeviceUpdate::default()).unwrap_err(),
            DeviceError::UnknownId("ghost".into())
        );
    }

    #[test]
    fn remove_device() {
        let reg = DeviceRegistry::new();
        let d = reg.get_or_create("e", &info(&[("hue", "x")])).expect("d");
        assert!(reg.remove(&d.id).is_some());
        assert!(reg.get(&d.id).is_none());
        assert!(reg.remove(&d.id).is_none());
    }
}
