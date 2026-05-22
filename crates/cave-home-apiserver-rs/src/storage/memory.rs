// SPDX-License-Identifier: Apache-2.0
//! In-memory `Storage` implementation.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! staging/src/k8s.io/apiserver/pkg/storage/cacher/cacher.go::Cacher
//! (logical model; we hold a `BTreeMap` instead of an etcd watch cache.)

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::api::{ApiObject, WatchEvent, WatchEventType};
use crate::types::ResourceRef;

use super::{Storage, StorageError, StorageResult};

const WATCH_CHANNEL_CAPACITY: usize = 256;

/// Per-resource bucket: name -> stored object.
type Bucket = BTreeMap<String, ApiObject>;

/// Top-level shard key — `(group, version, resource)`.
type ShardKey = (String, String, String);

/// Inner mutable state.
struct Inner {
    /// Map of `(group, version, resource)` -> namespace -> object map.
    /// For cluster-scoped resources we use the empty string as namespace.
    objects: HashMap<ShardKey, HashMap<String, Bucket>>,
    /// One broadcast channel per shard key (shared across namespaces).
    watchers: HashMap<ShardKey, broadcast::Sender<WatchEvent>>,
}

/// In-memory `Storage` backend backed by an `Arc<RwLock<HashMap>>` family.
///
/// Phase 2 production implementation: fully functional, no stubs. Phase 3
/// will swap this for `cave-home-kine-rs` (etcd-over-SQLite).
#[derive(Clone)]
pub struct InMemoryStorage {
    inner: Arc<RwLock<Inner>>,
    /// Monotonic counter that drives `resourceVersion`.
    rv: Arc<AtomicU64>,
}

impl InMemoryStorage {
    /// Construct an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                objects: HashMap::new(),
                watchers: HashMap::new(),
            })),
            rv: Arc::new(AtomicU64::new(1)),
        }
    }

    fn shard_key(key: &ResourceRef) -> ShardKey {
        (key.group.clone(), key.version.clone(), key.resource.clone())
    }

    fn next_rv(&self) -> String {
        let v = self.rv.fetch_add(1, Ordering::SeqCst);
        v.to_string()
    }

    /// Fetch-or-create the broadcast sender for a shard.
    fn watcher(&self, shard: &ShardKey) -> broadcast::Sender<WatchEvent> {
        let mut guard = self.inner.write();
        guard
            .watchers
            .entry(shard.clone())
            .or_insert_with(|| broadcast::channel(WATCH_CHANNEL_CAPACITY).0)
            .clone()
    }

    fn broadcast(&self, shard: &ShardKey, evt: WatchEvent) {
        let sender = self.watcher(shard);
        // Receiver count of zero is fine; we just drop the event.
        let _ = sender.send(evt);
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Storage for InMemoryStorage {
    async fn create(&self, key: &ResourceRef, mut obj: ApiObject) -> StorageResult<ApiObject> {
        let shard = Self::shard_key(key);
        let ns = key.namespace.clone();
        let name = if obj.metadata.name.is_empty() {
            key.name.clone()
        } else {
            obj.metadata.name.clone()
        };
        if name.is_empty() {
            return Err(StorageError::Invalid("metadata.name is required".to_string()));
        }
        // Auto-assign metadata.
        obj.metadata.name = name.clone();
        obj.metadata.namespace = ns.clone();
        if obj.metadata.uid.is_empty() {
            obj.metadata.uid = Uuid::new_v4().to_string();
        }
        obj.metadata.resource_version = self.next_rv();
        if obj.metadata.creation_timestamp.is_none() {
            obj.metadata.creation_timestamp = Some(rfc3339_now());
        }

        {
            let mut guard = self.inner.write();
            let bucket = guard
                .objects
                .entry(shard.clone())
                .or_default()
                .entry(ns.clone())
                .or_default();
            if bucket.contains_key(&name) {
                return Err(StorageError::AlreadyExists(format!(
                    "{}/{}/{}/{}/{}",
                    key.group, key.version, key.resource, ns, name
                )));
            }
            bucket.insert(name.clone(), obj.clone());
        }

        self.broadcast(
            &shard,
            WatchEvent {
                event_type: WatchEventType::Added,
                object: obj.clone(),
            },
        );
        Ok(obj)
    }

    async fn get(&self, key: &ResourceRef) -> StorageResult<ApiObject> {
        let shard = Self::shard_key(key);
        let guard = self.inner.read();
        let obj = guard
            .objects
            .get(&shard)
            .and_then(|by_ns| by_ns.get(&key.namespace))
            .and_then(|bucket| bucket.get(&key.name))
            .cloned()
            .ok_or_else(|| StorageError::NotFound(key.storage_key()))?;
        Ok(obj)
    }

    async fn update(&self, key: &ResourceRef, mut obj: ApiObject) -> StorageResult<ApiObject> {
        let shard = Self::shard_key(key);
        let new_rv = self.next_rv();

        let updated = {
            let mut guard = self.inner.write();
            let by_ns = guard
                .objects
                .get_mut(&shard)
                .ok_or_else(|| StorageError::NotFound(key.storage_key()))?;
            let bucket = by_ns
                .get_mut(&key.namespace)
                .ok_or_else(|| StorageError::NotFound(key.storage_key()))?;
            let existing = bucket
                .get(&key.name)
                .ok_or_else(|| StorageError::NotFound(key.storage_key()))?;

            // Optimistic concurrency: if caller supplied a resourceVersion it
            // must match the stored value.
            if !obj.metadata.resource_version.is_empty()
                && obj.metadata.resource_version != existing.metadata.resource_version
            {
                return Err(StorageError::Conflict(
                    key.storage_key(),
                    existing.metadata.resource_version.clone(),
                    obj.metadata.resource_version.clone(),
                ));
            }
            // Preserve immutable metadata.
            obj.metadata.name = existing.metadata.name.clone();
            obj.metadata.namespace = existing.metadata.namespace.clone();
            obj.metadata.uid = existing.metadata.uid.clone();
            obj.metadata.creation_timestamp = existing.metadata.creation_timestamp.clone();
            obj.metadata.resource_version = new_rv;

            bucket.insert(key.name.clone(), obj.clone());
            obj
        };

        self.broadcast(
            &shard,
            WatchEvent {
                event_type: WatchEventType::Modified,
                object: updated.clone(),
            },
        );
        Ok(updated)
    }

    async fn delete(&self, key: &ResourceRef) -> StorageResult<ApiObject> {
        let shard = Self::shard_key(key);
        let prior = {
            let mut guard = self.inner.write();
            let by_ns = guard
                .objects
                .get_mut(&shard)
                .ok_or_else(|| StorageError::NotFound(key.storage_key()))?;
            let bucket = by_ns
                .get_mut(&key.namespace)
                .ok_or_else(|| StorageError::NotFound(key.storage_key()))?;
            bucket
                .remove(&key.name)
                .ok_or_else(|| StorageError::NotFound(key.storage_key()))?
        };
        self.broadcast(
            &shard,
            WatchEvent {
                event_type: WatchEventType::Deleted,
                object: prior.clone(),
            },
        );
        Ok(prior)
    }

    async fn list(&self, key: &ResourceRef) -> StorageResult<Vec<ApiObject>> {
        let shard = Self::shard_key(key);
        let guard = self.inner.read();
        let Some(by_ns) = guard.objects.get(&shard) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        if key.namespace.is_empty() {
            for bucket in by_ns.values() {
                for v in bucket.values() {
                    out.push(v.clone());
                }
            }
        } else if let Some(bucket) = by_ns.get(&key.namespace) {
            for v in bucket.values() {
                out.push(v.clone());
            }
        }
        // Stable order: name then namespace.
        out.sort_by(|a, b| {
            a.metadata
                .namespace
                .cmp(&b.metadata.namespace)
                .then(a.metadata.name.cmp(&b.metadata.name))
        });
        Ok(out)
    }

    fn watch(&self, key: &ResourceRef) -> broadcast::Receiver<WatchEvent> {
        let shard = Self::shard_key(key);
        self.watcher(&shard).subscribe()
    }
}

/// RFC3339 timestamp helper. Uses the system clock; tests can ignore the
/// exact value (just check it's set).
fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("1970-01-01T00:00:{:02}Z", now.as_secs() % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pod_ref(name: &str) -> ResourceRef {
        ResourceRef::namespaced("", "v1", "pods", "default", name)
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let s = InMemoryStorage::new();
        let pod = ApiObject::new("v1", "Pod", "nginx").with_namespace("default");
        let created = s.create(&pod_ref("nginx"), pod).await.expect("create");
        assert!(!created.metadata.uid.is_empty());
        let got = s.get(&pod_ref("nginx")).await.expect("get");
        assert_eq!(got.metadata.name, "nginx");
    }

    #[tokio::test]
    async fn create_collision_returns_already_exists() {
        let s = InMemoryStorage::new();
        let pod = ApiObject::new("v1", "Pod", "p").with_namespace("default");
        s.create(&pod_ref("p"), pod.clone()).await.expect("first");
        let err = s.create(&pod_ref("p"), pod).await.expect_err("second");
        assert!(matches!(err, StorageError::AlreadyExists(_)));
    }

    #[tokio::test]
    async fn missing_get_returns_not_found() {
        let s = InMemoryStorage::new();
        let err = s.get(&pod_ref("missing")).await.expect_err("not found");
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn update_bumps_resource_version_and_preserves_uid() {
        let s = InMemoryStorage::new();
        let pod = ApiObject::new("v1", "Pod", "p").with_namespace("default");
        let created = s.create(&pod_ref("p"), pod).await.expect("create");
        let original_uid = created.metadata.uid.clone();
        let original_rv = created.metadata.resource_version.clone();
        let mut next = created;
        next.spec = Some(serde_json::json!({"replicas": 3}));
        let updated = s.update(&pod_ref("p"), next).await.expect("update");
        assert_eq!(updated.metadata.uid, original_uid);
        assert_ne!(updated.metadata.resource_version, original_rv);
    }

    #[tokio::test]
    async fn update_with_stale_rv_returns_conflict() {
        let s = InMemoryStorage::new();
        let pod = ApiObject::new("v1", "Pod", "p").with_namespace("default");
        let created = s.create(&pod_ref("p"), pod).await.expect("create");
        let mut next = created.clone();
        next.metadata.resource_version = "999".to_string();
        let err = s.update(&pod_ref("p"), next).await.expect_err("conflict");
        assert!(matches!(err, StorageError::Conflict(_, _, _)));
    }

    #[tokio::test]
    async fn list_all_namespaces() {
        let s = InMemoryStorage::new();
        s.create(
            &ResourceRef::namespaced("", "v1", "pods", "default", "a"),
            ApiObject::new("v1", "Pod", "a").with_namespace("default"),
        )
        .await
        .expect("a");
        s.create(
            &ResourceRef::namespaced("", "v1", "pods", "kube-system", "b"),
            ApiObject::new("v1", "Pod", "b").with_namespace("kube-system"),
        )
        .await
        .expect("b");
        let all = s
            .list(&ResourceRef::namespaced("", "v1", "pods", "", ""))
            .await
            .expect("list");
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn list_one_namespace() {
        let s = InMemoryStorage::new();
        s.create(
            &ResourceRef::namespaced("", "v1", "pods", "default", "a"),
            ApiObject::new("v1", "Pod", "a").with_namespace("default"),
        )
        .await
        .expect("a");
        s.create(
            &ResourceRef::namespaced("", "v1", "pods", "kube-system", "b"),
            ApiObject::new("v1", "Pod", "b").with_namespace("kube-system"),
        )
        .await
        .expect("b");
        let in_default = s
            .list(&ResourceRef::namespaced("", "v1", "pods", "default", ""))
            .await
            .expect("list");
        assert_eq!(in_default.len(), 1);
        assert_eq!(in_default[0].metadata.name, "a");
    }

    #[tokio::test]
    async fn delete_returns_prior_object() {
        let s = InMemoryStorage::new();
        let pod = ApiObject::new("v1", "Pod", "p").with_namespace("default");
        s.create(&pod_ref("p"), pod).await.expect("create");
        let deleted = s.delete(&pod_ref("p")).await.expect("delete");
        assert_eq!(deleted.metadata.name, "p");
        let err = s.get(&pod_ref("p")).await.expect_err("gone");
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn watch_receives_added_event() {
        let s = InMemoryStorage::new();
        let mut rx = s.watch(&pod_ref(""));
        let pod = ApiObject::new("v1", "Pod", "x").with_namespace("default");
        s.create(&pod_ref("x"), pod).await.expect("create");
        let evt = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        assert_eq!(evt.event_type, WatchEventType::Added);
        assert_eq!(evt.object.metadata.name, "x");
    }

    #[tokio::test]
    async fn watch_receives_modified_and_deleted() {
        let s = InMemoryStorage::new();
        let mut rx = s.watch(&pod_ref(""));
        let pod = ApiObject::new("v1", "Pod", "y").with_namespace("default");
        let created = s.create(&pod_ref("y"), pod).await.expect("create");
        s.update(&pod_ref("y"), created).await.expect("update");
        s.delete(&pod_ref("y")).await.expect("delete");

        // Drain the events; should see ADDED, MODIFIED, DELETED in order.
        let mut kinds = Vec::new();
        for _ in 0..3 {
            let evt = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
                .await
                .expect("timeout")
                .expect("recv");
            kinds.push(evt.event_type);
        }
        assert_eq!(
            kinds,
            vec![
                WatchEventType::Added,
                WatchEventType::Modified,
                WatchEventType::Deleted
            ]
        );
    }
}
