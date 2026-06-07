// SPDX-License-Identifier: Apache-2.0
//! Storage backend seam — the apiserver ↔ etcd/kine boundary.
//!
//! The decision core's [`crate::registry::Registry`] is an in-memory store that
//! gives full object-level REST semantics. For a *persistent* apiserver those
//! objects must live in etcd; k3s replaces etcd with **kine**, an
//! etcd-compatible MVCC datastore. This module defines the narrow KV contract
//! the apiserver storage layer speaks ([`Backend`]) and binds it to
//! `cave-home-kine-rs`'s [`cave_home_kine_rs::Store`] ([`KineBackend`]) — the
//! "kine client" the task calls for. Object bodies are stored as JSON bytes
//! under the documented etcd registry key (`/registry/<resource>/<ns>/<name>`)
//! and kine's monotonic revision is surfaced as the Kubernetes
//! `resourceVersion`.
//!
//! Behavioural reference: the etcd v3 KV contract the apiserver storage layer
//! relies on (conditional create, revisioned put, prefix range, delete) as
//! reimplemented by kine. Wiring [`Backend`] in as the registry's persistence
//! layer (so `ApiServer` writes survive a restart) is the next bolt-on; the
//! seam and a tested kine binding land here.

use cave_home_kine_rs::Store;

use crate::gvk::GroupVersionResource;
use crate::status::{Result, Status, StatusReason};

/// The etcd registry key for an object: `/registry/<resource>[/<ns>][/<name>]`.
/// With an empty `name` this is the collection *prefix* used by list/range.
#[must_use]
pub fn registry_key(gvr: &GroupVersionResource, namespace: &str, name: &str) -> String {
    let mut key = format!("/registry/{}", gvr.resource);
    if !namespace.is_empty() {
        key.push('/');
        key.push_str(namespace);
    }
    if !name.is_empty() {
        key.push('/');
        key.push_str(name);
    }
    key
}

/// The KV operations the apiserver storage layer needs, each returning the
/// store revision (→ `resourceVersion`). This is the seam: the in-memory
/// registry is the default, [`KineBackend`] the persistent binding, and a real
/// remote kine over gRPC would implement the same trait.
pub trait Backend {
    /// Conditionally insert `value` at `key` (fails `AlreadyExists` if a live
    /// row exists). Returns the new revision.
    ///
    /// # Errors
    /// `AlreadyExists` if the key is live; `InternalError` on a store fault.
    fn create(&mut self, key: &str, value: &[u8]) -> Result<u64>;

    /// Read the live value at `key`, or `None` if absent/deleted.
    ///
    /// # Errors
    /// `InternalError` on a store fault.
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Overwrite the live value at `key`. Returns the new revision.
    ///
    /// # Errors
    /// `NotFound` if the key has no live row; `InternalError` on a store fault.
    fn update(&mut self, key: &str, value: &[u8]) -> Result<u64>;

    /// Delete the key. Returns `true` if a live row was removed.
    ///
    /// # Errors
    /// `InternalError` on a store fault.
    fn delete(&mut self, key: &str) -> Result<bool>;

    /// List `(key, value)` of every live row under `prefix`, sorted by key.
    ///
    /// # Errors
    /// `InternalError` on a store fault.
    fn list(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>>;

    /// The current global store revision.
    fn revision(&self) -> u64;
}

/// A [`Backend`] backed by an embedded kine [`Store`] (k3s's datastore).
#[derive(Debug, Default)]
pub struct KineBackend {
    store: Store,
}

impl KineBackend {
    /// A backend over a fresh, empty kine store.
    #[must_use]
    pub fn new() -> Self {
        Self { store: Store::new() }
    }
}

fn internal(msg: impl std::fmt::Display) -> Status {
    Status::new(StatusReason::InternalError, format!("kine backend error: {msg}"))
}

#[allow(clippy::cast_sign_loss)] // kine revisions are monotonic and non-negative
impl Backend for KineBackend {
    fn create(&mut self, key: &str, value: &[u8]) -> Result<u64> {
        match self.store.create(key.as_bytes(), value, 0).map_err(internal)? {
            Some(rev) => Ok(rev as u64),
            None => Err(Status::already_exists(format!("key {key:?} already exists"))),
        }
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.store.get_live(key.as_bytes()).map(|r| r.value.clone()))
    }

    fn update(&mut self, key: &str, value: &[u8]) -> Result<u64> {
        match self.store.update(key.as_bytes(), value, 0).map_err(internal)? {
            Some(rev) => Ok(rev as u64),
            None => Err(Status::not_found(format!("key {key:?} not found"))),
        }
    }

    fn delete(&mut self, key: &str) -> Result<bool> {
        Ok(self.store.delete(key.as_bytes()).map_err(internal)?.is_some())
    }

    fn list(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let mut out = Vec::new();
        for key in self.store.live_keys() {
            if key.starts_with(prefix.as_bytes()) {
                if let Some(row) = self.store.get_live(&key) {
                    let key_str = String::from_utf8_lossy(&key).into_owned();
                    out.push((key_str, row.value.clone()));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    fn revision(&self) -> u64 {
        self.store
            .rows()
            .iter()
            .map(|r| r.mod_revision)
            .max()
            .unwrap_or(0)
            .max(0) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gvk::GroupVersionResource;

    fn pods() -> GroupVersionResource {
        GroupVersionResource::new("", "v1", "pods")
    }

    #[test]
    fn registry_key_formats_namespaced_cluster_and_prefix() {
        assert_eq!(
            registry_key(&pods(), "default", "nginx"),
            "/registry/pods/default/nginx"
        );
        let nodes = GroupVersionResource::new("", "v1", "nodes");
        assert_eq!(registry_key(&nodes, "", "worker-1"), "/registry/nodes/worker-1");
        // Collection prefix (no name).
        assert_eq!(registry_key(&pods(), "default", ""), "/registry/pods/default");
    }

    #[test]
    fn create_then_get_round_trips() {
        let mut b = KineBackend::new();
        let key = registry_key(&pods(), "default", "nginx");
        let rev = b.create(&key, b"{\"kind\":\"Pod\"}").expect("create");
        assert!(rev >= 1);
        assert_eq!(b.get(&key).expect("get").as_deref(), Some(&b"{\"kind\":\"Pod\"}"[..]));
        assert_eq!(b.revision(), rev);
    }

    #[test]
    fn create_duplicate_is_already_exists() {
        let mut b = KineBackend::new();
        let key = registry_key(&pods(), "default", "nginx");
        b.create(&key, b"a").expect("first");
        let err = b.create(&key, b"b").expect_err("dup");
        assert_eq!(err.reason, StatusReason::AlreadyExists);
    }

    #[test]
    fn update_changes_value_and_bumps_revision() {
        let mut b = KineBackend::new();
        let key = registry_key(&pods(), "default", "nginx");
        let r1 = b.create(&key, b"v1").expect("create");
        let r2 = b.update(&key, b"v2").expect("update");
        assert!(r2 > r1);
        assert_eq!(b.get(&key).expect("get").as_deref(), Some(&b"v2"[..]));
    }

    #[test]
    fn update_missing_is_not_found() {
        let mut b = KineBackend::new();
        let err = b.update("/registry/pods/default/ghost", b"x").expect_err("missing");
        assert_eq!(err.reason, StatusReason::NotFound);
    }

    #[test]
    fn delete_removes_live_row() {
        let mut b = KineBackend::new();
        let key = registry_key(&pods(), "default", "nginx");
        b.create(&key, b"v").expect("create");
        assert!(b.delete(&key).expect("delete"));
        assert!(b.get(&key).expect("get").is_none());
        // Deleting again reports no live row removed.
        assert!(!b.delete(&key).expect("delete2"));
    }

    #[test]
    fn list_by_prefix_returns_sorted_live_rows() {
        let mut b = KineBackend::new();
        b.create(&registry_key(&pods(), "default", "b"), b"B").expect("b");
        b.create(&registry_key(&pods(), "default", "a"), b"A").expect("a");
        b.create(&registry_key(&pods(), "other", "c"), b"C").expect("c");
        let in_default = b.list("/registry/pods/default").expect("list");
        let keys: Vec<&str> = in_default.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["/registry/pods/default/a", "/registry/pods/default/b"]);
        // The whole-resource prefix sees all three.
        assert_eq!(b.list("/registry/pods").expect("all").len(), 3);
    }
}
