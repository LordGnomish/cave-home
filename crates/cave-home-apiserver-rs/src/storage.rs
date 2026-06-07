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
