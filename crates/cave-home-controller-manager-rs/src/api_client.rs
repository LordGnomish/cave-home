// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         staging/src/k8s.io/client-go/kubernetes/typed/*/v1/* (generic CRUD)
//
//! API-client abstraction used by every controller.
//!
//! Phase 2 deliberately defines its OWN [`ControllerApiClient`] trait rather
//! than depending on `cave-home-apiserver-rs` — keeping the controller-manager
//! crate compilable in isolation is a strict invariant of the multi-crate
//! split (ADR-004 §4 "inter-crate decoupling"). The wiring to the real
//! apiserver client lives one layer up.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::types::{KubeResource, Uid};

/// Errors raised by [`ControllerApiClient`]. Mirrors a tiny subset of
/// `k8s.io/apimachinery/pkg/api/errors/errors.go`.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ApiError {
    #[error("not found: {kind} {namespace:?}/{name}")]
    NotFound {
        kind: String,
        namespace: Option<String>,
        name: String,
    },
    #[error("conflict: {kind} {namespace:?}/{name} (resource version mismatch)")]
    Conflict {
        kind: String,
        namespace: Option<String>,
        name: String,
    },
    #[error("already exists: {kind} {namespace:?}/{name}")]
    AlreadyExists {
        kind: String,
        namespace: Option<String>,
        name: String,
    },
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type ApiResult<T> = Result<T, ApiError>;

/// `watch.Event.Type`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchEventType {
    Added,
    Modified,
    Deleted,
}

/// `watch.Event` parameterised over the resource being watched.
#[derive(Clone, Debug)]
pub struct WatchEvent<T> {
    pub event: WatchEventType,
    pub object: T,
}

/// Stream of [`WatchEvent`]s. Implemented as a tokio broadcast receiver so
/// multiple controllers can share one logical informer per type.
pub type EventStream<T> = broadcast::Receiver<WatchEvent<T>>;

/// The controller-side API client trait.
///
/// Concrete implementations live outside this crate; see
/// [`InMemoryApiClient`] for the test impl that backs every Phase 2 test.
#[async_trait]
pub trait ControllerApiClient: Send + Sync {
    async fn list<T: KubeResource>(
        &self,
        namespace: Option<&str>,
        label_selector: Option<&LabelSelectorFilter>,
    ) -> ApiResult<Vec<T>>;

    async fn get<T: KubeResource>(&self, namespace: Option<&str>, name: &str) -> ApiResult<T>;

    async fn create<T: KubeResource>(&self, namespace: Option<&str>, obj: T) -> ApiResult<T>;

    async fn update<T: KubeResource>(&self, namespace: Option<&str>, obj: T) -> ApiResult<T>;

    async fn delete(&self, kind: &str, namespace: Option<&str>, name: &str) -> ApiResult<()>;

    async fn watch<T: KubeResource>(&self, namespace: Option<&str>) -> ApiResult<EventStream<T>>;
}

/// Subset of [`crate::types::LabelSelector`] passed to `list`.
///
/// Modelled as its own type so callers can build it without owning a full
/// [`crate::types::LabelSelector`] (e.g. controllers that compute selectors
/// per-pod from labels they observed).
#[derive(Clone, Debug, Default)]
pub struct LabelSelectorFilter {
    pub match_labels: BTreeMap<String, String>,
}

impl LabelSelectorFilter {
    #[must_use]
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        self.match_labels
            .iter()
            .all(|(k, v)| labels.get(k).map_or(false, |actual| actual == v))
    }
}

impl From<&crate::types::LabelSelector> for LabelSelectorFilter {
    fn from(sel: &crate::types::LabelSelector) -> Self {
        Self {
            match_labels: sel.match_labels.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// InMemoryApiClient — test implementation
// ---------------------------------------------------------------------------

/// Maximum number of outstanding events buffered per watch channel.
const WATCH_CHANNEL_CAPACITY: usize = 256;

/// Per-kind in-memory store. One bucket per resource kind ⨯ namespace.
///
/// Keys are `(namespace, name)`; cluster-scoped resources use an empty
/// namespace string. Values are stored as boxed type-erased payloads keyed
/// by `TypeId` of [`KubeResource`] implementor.
#[derive(Default)]
struct Bucket {
    by_name: BTreeMap<(String, String), TypeErased>,
    /// Watchers receive events for resources in this bucket.
    watcher: Option<broadcast::Sender<TypeErasedEvent>>,
}

/// A type-erased clone of a [`KubeResource`] — `Arc<dyn Any>`.
#[derive(Clone)]
struct TypeErased(Arc<dyn std::any::Any + Send + Sync>);

#[derive(Clone)]
struct TypeErasedEvent {
    event: WatchEventType,
    object: TypeErased,
}

impl TypeErased {
    fn new<T: KubeResource>(value: T) -> Self {
        Self(Arc::new(value))
    }

    fn downcast<T: KubeResource>(&self) -> Option<T> {
        self.0.downcast_ref::<T>().cloned()
    }
}

/// In-memory implementation of [`ControllerApiClient`] used by every test in
/// the controller-manager crate.
///
/// Resource version starts at 1 and increments on every successful mutation.
#[derive(Clone, Default)]
pub struct InMemoryApiClient {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    buckets: BTreeMap<&'static str, Bucket>,
    next_rv: u64,
    next_uid: u64,
}

impl InMemoryApiClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Synchronously seed an object — useful in test fixtures (no `await`).
    ///
    /// Always succeeds. Panics if the object would clash with an existing
    /// `(namespace, name)` — callers are tests and a clash is a test bug.
    pub fn seed<T: KubeResource>(&self, namespace: Option<&str>, mut obj: T) -> T {
        let mut inner = self.inner.lock();
        inner.next_rv += 1;
        let rv = inner.next_rv;
        if obj.uid().0.is_empty() {
            inner.next_uid += 1;
            obj.meta_mut().uid = Uid::new(format!("uid-{}", inner.next_uid));
        }
        obj.meta_mut().resource_version = rv;
        if obj.meta().generation == 0 {
            obj.meta_mut().generation = 1;
        }
        let kind = T::kind();
        let ns = namespace.unwrap_or("").to_string();
        let name = obj.meta().name.clone();
        let bucket = inner.buckets.entry(kind).or_default();
        let key = (ns, name);
        assert!(
            !bucket.by_name.contains_key(&key),
            "InMemoryApiClient::seed: duplicate {} {:?}",
            kind,
            key
        );
        let erased = TypeErased::new(obj.clone());
        bucket.by_name.insert(key, erased.clone());
        if let Some(tx) = &bucket.watcher {
            let _ = tx.send(TypeErasedEvent {
                event: WatchEventType::Added,
                object: erased,
            });
        }
        obj
    }

    /// Snapshot count of objects of `kind` across all namespaces.
    #[must_use]
    pub fn count(&self, kind: &'static str) -> usize {
        self.inner
            .lock()
            .buckets
            .get(kind)
            .map_or(0, |b| b.by_name.len())
    }
}

#[async_trait]
impl ControllerApiClient for InMemoryApiClient {
    async fn list<T: KubeResource>(
        &self,
        namespace: Option<&str>,
        label_selector: Option<&LabelSelectorFilter>,
    ) -> ApiResult<Vec<T>> {
        let inner = self.inner.lock();
        let Some(bucket) = inner.buckets.get(T::kind()) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for ((ns, _name), erased) in &bucket.by_name {
            if let Some(target) = namespace {
                if ns != target {
                    continue;
                }
            }
            let Some(obj) = erased.downcast::<T>() else {
                continue;
            };
            if let Some(sel) = label_selector {
                if !sel.matches(obj.labels()) {
                    continue;
                }
            }
            out.push(obj);
        }
        Ok(out)
    }

    async fn get<T: KubeResource>(&self, namespace: Option<&str>, name: &str) -> ApiResult<T> {
        let inner = self.inner.lock();
        let Some(bucket) = inner.buckets.get(T::kind()) else {
            return Err(ApiError::NotFound {
                kind: T::kind().to_string(),
                namespace: namespace.map(String::from),
                name: name.to_string(),
            });
        };
        let key = (namespace.unwrap_or("").to_string(), name.to_string());
        bucket.by_name.get(&key).and_then(|e| e.downcast::<T>()).ok_or_else(|| ApiError::NotFound {
            kind: T::kind().to_string(),
            namespace: namespace.map(String::from),
            name: name.to_string(),
        })
    }

    async fn create<T: KubeResource>(&self, namespace: Option<&str>, mut obj: T) -> ApiResult<T> {
        let mut inner = self.inner.lock();
        let kind = T::kind();
        let ns = namespace.unwrap_or("").to_string();
        let name = obj.meta().name.clone();
        let key = (ns.clone(), name.clone());
        let bucket_exists = inner
            .buckets
            .get(kind)
            .map_or(false, |b| b.by_name.contains_key(&key));
        if bucket_exists {
            return Err(ApiError::AlreadyExists {
                kind: kind.to_string(),
                namespace: namespace.map(String::from),
                name,
            });
        }
        inner.next_rv += 1;
        let rv = inner.next_rv;
        if obj.uid().0.is_empty() {
            inner.next_uid += 1;
            obj.meta_mut().uid = Uid::new(format!("uid-{}", inner.next_uid));
        }
        obj.meta_mut().resource_version = rv;
        if obj.meta().generation == 0 {
            obj.meta_mut().generation = 1;
        }
        // Namespace is part of the key path the test uses, force it on the
        // metadata too so list+get can find it back.
        if obj.meta().namespace.is_empty() {
            obj.meta_mut().namespace = ns.clone();
        }
        let bucket = inner.buckets.entry(kind).or_default();
        let erased = TypeErased::new(obj.clone());
        bucket.by_name.insert(key, erased.clone());
        if let Some(tx) = &bucket.watcher {
            let _ = tx.send(TypeErasedEvent {
                event: WatchEventType::Added,
                object: erased,
            });
        }
        Ok(obj)
    }

    async fn update<T: KubeResource>(&self, namespace: Option<&str>, mut obj: T) -> ApiResult<T> {
        let mut inner = self.inner.lock();
        let kind = T::kind();
        let ns = namespace.unwrap_or("").to_string();
        let name = obj.meta().name.clone();
        let key = (ns, name.clone());
        let bucket = inner.buckets.entry(kind).or_default();
        let Some(prev) = bucket.by_name.get(&key).and_then(|e| e.downcast::<T>()) else {
            return Err(ApiError::NotFound {
                kind: kind.to_string(),
                namespace: namespace.map(String::from),
                name,
            });
        };
        if obj.meta().resource_version != 0
            && obj.meta().resource_version < prev.meta().resource_version
        {
            return Err(ApiError::Conflict {
                kind: kind.to_string(),
                namespace: namespace.map(String::from),
                name,
            });
        }
        inner.next_rv += 1;
        let new_rv = inner.next_rv;
        obj.meta_mut().resource_version = new_rv;
        if obj.uid().0.is_empty() {
            obj.meta_mut().uid = prev.uid().clone();
        }
        let bucket = inner.buckets.entry(kind).or_default();
        let erased = TypeErased::new(obj.clone());
        bucket.by_name.insert(key, erased.clone());
        if let Some(tx) = &bucket.watcher {
            let _ = tx.send(TypeErasedEvent {
                event: WatchEventType::Modified,
                object: erased,
            });
        }
        Ok(obj)
    }

    async fn delete(&self, kind: &str, namespace: Option<&str>, name: &str) -> ApiResult<()> {
        let mut inner = self.inner.lock();
        let Some(bucket) = inner.buckets.get_mut(kind) else {
            return Err(ApiError::NotFound {
                kind: kind.to_string(),
                namespace: namespace.map(String::from),
                name: name.to_string(),
            });
        };
        let key = (namespace.unwrap_or("").to_string(), name.to_string());
        let Some(erased) = bucket.by_name.remove(&key) else {
            return Err(ApiError::NotFound {
                kind: kind.to_string(),
                namespace: namespace.map(String::from),
                name: name.to_string(),
            });
        };
        if let Some(tx) = &bucket.watcher {
            let _ = tx.send(TypeErasedEvent {
                event: WatchEventType::Deleted,
                object: erased,
            });
        }
        Ok(())
    }

    async fn watch<T: KubeResource>(
        &self,
        _namespace: Option<&str>,
    ) -> ApiResult<EventStream<T>> {
        // Build the per-kind broadcaster lazily and tee its erased events out
        // to a typed receiver so callers see exactly `WatchEvent<T>`.
        let mut inner = self.inner.lock();
        let bucket = inner.buckets.entry(T::kind()).or_default();
        let tx_erased = match &bucket.watcher {
            Some(tx) => tx.clone(),
            None => {
                let (tx, _rx) = broadcast::channel(WATCH_CHANNEL_CAPACITY);
                bucket.watcher = Some(tx.clone());
                tx
            }
        };
        let mut rx_erased = tx_erased.subscribe();
        drop(inner);
        let (tx_typed, rx_typed) = broadcast::channel::<WatchEvent<T>>(WATCH_CHANNEL_CAPACITY);
        // Forwarder task — lives until the typed channel has no subscribers.
        tokio::spawn(async move {
            loop {
                match rx_erased.recv().await {
                    Ok(ev) => {
                        if let Some(obj) = ev.object.downcast::<T>() {
                            if tx_typed
                                .send(WatchEvent {
                                    event: ev.event,
                                    object: obj,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(rx_typed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Pod, ReplicaSet};

    fn pod(ns: &str, name: &str) -> Pod {
        let mut p = Pod::default();
        p.metadata.name = name.into();
        p.metadata.namespace = ns.into();
        p
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let c = InMemoryApiClient::new();
        let p = c.create::<Pod>(Some("default"), pod("default", "p1")).await.unwrap();
        let got = c.get::<Pod>(Some("default"), "p1").await.unwrap();
        assert_eq!(p.uid(), got.uid());
        assert!(p.uid().as_str().starts_with("uid-"));
        assert_eq!(p.meta().resource_version, got.meta().resource_version);
    }

    #[tokio::test]
    async fn create_returns_already_exists_on_duplicate() {
        let c = InMemoryApiClient::new();
        c.create::<Pod>(Some("default"), pod("default", "p1")).await.unwrap();
        let err = c
            .create::<Pod>(Some("default"), pod("default", "p1"))
            .await
            .unwrap_err();
        assert!(matches!(err, ApiError::AlreadyExists { .. }));
    }

    #[tokio::test]
    async fn list_filters_by_namespace_and_labels() {
        let c = InMemoryApiClient::new();
        let mut p = pod("ns1", "p1");
        p.metadata.labels.insert("app".into(), "nginx".into());
        c.create::<Pod>(Some("ns1"), p).await.unwrap();
        let mut p2 = pod("ns2", "p2");
        p2.metadata.labels.insert("app".into(), "redis".into());
        c.create::<Pod>(Some("ns2"), p2).await.unwrap();

        let in_ns1: Vec<Pod> = c.list::<Pod>(Some("ns1"), None).await.unwrap();
        assert_eq!(in_ns1.len(), 1);

        let mut filter = LabelSelectorFilter::default();
        filter.match_labels.insert("app".into(), "nginx".into());
        let nginx_pods: Vec<Pod> = c.list::<Pod>(None, Some(&filter)).await.unwrap();
        assert_eq!(nginx_pods.len(), 1);
        assert_eq!(nginx_pods[0].meta().name, "p1");
    }

    #[tokio::test]
    async fn update_advances_resource_version() {
        let c = InMemoryApiClient::new();
        let mut p = c
            .create::<Pod>(Some("default"), pod("default", "p1"))
            .await
            .unwrap();
        let before = p.meta().resource_version;
        p.metadata.labels.insert("k".into(), "v".into());
        let updated = c.update::<Pod>(Some("default"), p).await.unwrap();
        assert!(updated.meta().resource_version > before);
    }

    #[tokio::test]
    async fn delete_then_get_returns_not_found() {
        let c = InMemoryApiClient::new();
        c.create::<Pod>(Some("default"), pod("default", "p1")).await.unwrap();
        c.delete("Pod", Some("default"), "p1").await.unwrap();
        let err = c.get::<Pod>(Some("default"), "p1").await.unwrap_err();
        assert!(matches!(err, ApiError::NotFound { .. }));
    }

    #[tokio::test]
    async fn watch_observes_create_modify_delete() {
        let c = InMemoryApiClient::new();
        let mut rx = c.watch::<Pod>(None).await.unwrap();
        let p = c
            .create::<Pod>(Some("default"), pod("default", "p1"))
            .await
            .unwrap();
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.event, WatchEventType::Added);
        assert_eq!(ev.object.meta().name, "p1");

        let mut p2 = p.clone();
        p2.metadata.labels.insert("k".into(), "v".into());
        c.update::<Pod>(Some("default"), p2).await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.event, WatchEventType::Modified);

        c.delete("Pod", Some("default"), "p1").await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.event, WatchEventType::Deleted);
    }

    #[tokio::test]
    async fn replicaset_is_namespaced_and_uid_assigned() {
        let c = InMemoryApiClient::new();
        let rs = c
            .create::<ReplicaSet>(Some("default"), ReplicaSet {
                metadata: crate::types::ObjectMeta {
                    name: "rs1".into(),
                    namespace: "default".into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(!rs.uid().as_str().is_empty());
    }
}
