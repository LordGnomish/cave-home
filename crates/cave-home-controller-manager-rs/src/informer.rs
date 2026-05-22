// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         staging/src/k8s.io/client-go/tools/cache/{shared_informer.go,reflector.go,thread_safe_store.go}
//         pkg/controller/informers/factory.go
//
//! Shared informer pattern.
//!
//! Mirrors `client-go/tools/cache.SharedInformer`: a single watch stream per
//! kind ⨯ namespace, fanned out to multiple event handlers, with a local
//! cache (the "store") of the latest observed objects keyed by
//! `namespace/name`.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::broadcast;

use crate::api_client::{ApiResult, ControllerApiClient, LabelSelectorFilter, WatchEventType};
use crate::types::KubeResource;

/// `meta.namespace_key_func` — `namespace/name` for namespaced objects,
/// `name` for cluster-scoped.
#[must_use]
pub fn meta_namespace_key<R: KubeResource>(obj: &R) -> String {
    let ns = obj.namespace();
    let name = obj.name();
    if ns.is_empty() {
        name.to_string()
    } else {
        format!("{ns}/{name}")
    }
}

/// Split `"namespace/name"` back into its parts. Returns `(namespace, name)`
/// — `namespace` is empty for cluster-scoped keys.
#[must_use]
pub fn split_meta_namespace_key(key: &str) -> (String, String) {
    match key.find('/') {
        Some(idx) => (key[..idx].to_string(), key[idx + 1..].to_string()),
        None => (String::new(), key.to_string()),
    }
}

/// Event delivered to an [`EventHandler`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InformerEvent {
    Add,
    Update,
    Delete,
}

impl From<WatchEventType> for InformerEvent {
    fn from(value: WatchEventType) -> Self {
        match value {
            WatchEventType::Added => Self::Add,
            WatchEventType::Modified => Self::Update,
            WatchEventType::Deleted => Self::Delete,
        }
    }
}

/// Snapshot store inside an informer — the local cache that `lister.go`
/// upstream wraps.
#[derive(Default)]
pub struct Store<R: KubeResource> {
    inner: Mutex<HashMap<String, R>>,
}

impl<R: KubeResource> Store<R> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn insert(&self, obj: R) {
        let key = meta_namespace_key(&obj);
        self.inner.lock().insert(key, obj);
    }

    pub fn remove(&self, key: &str) {
        self.inner.lock().remove(key);
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<R> {
        self.inner.lock().get(key).cloned()
    }

    #[must_use]
    pub fn list(&self) -> Vec<R> {
        self.inner.lock().values().cloned().collect()
    }

    #[must_use]
    pub fn list_keys(&self) -> Vec<String> {
        self.inner.lock().keys().cloned().collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

/// Shared informer for one resource type.
///
/// Built via [`SharedInformer::start`] — the constructor seeds the local
/// store from a `list()` call and then forwards every `watch()` event into
/// both the store and any subscribed handlers.
pub struct SharedInformer<R: KubeResource> {
    store: Arc<Store<R>>,
    bus: broadcast::Sender<(InformerEvent, R)>,
    _phantom: PhantomData<R>,
}

impl<R: KubeResource> Clone for SharedInformer<R> {
    fn clone(&self) -> Self {
        Self {
            store: Arc::clone(&self.store),
            bus: self.bus.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<R: KubeResource> SharedInformer<R> {
    /// Construct, prime the store, and spawn the watch-forwarder task.
    pub async fn start<C: ControllerApiClient + Clone + 'static>(
        client: C,
        namespace: Option<&str>,
    ) -> ApiResult<Self> {
        Self::start_with_selector(client, namespace, None).await
    }

    pub async fn start_with_selector<C: ControllerApiClient + Clone + 'static>(
        client: C,
        namespace: Option<&str>,
        selector: Option<LabelSelectorFilter>,
    ) -> ApiResult<Self> {
        let store = Arc::new(Store::<R>::new());
        // Prime the store with a list().
        let primed: Vec<R> = client.list::<R>(namespace, selector.as_ref()).await?;
        for obj in &primed {
            store.insert(obj.clone());
        }
        let (tx, _rx) = broadcast::channel::<(InformerEvent, R)>(256);
        let ns_owned = namespace.map(String::from);
        // Watch stream forwarder.
        let mut rx = client.watch::<R>(namespace).await?;
        let store_clone = Arc::clone(&store);
        let tx_clone = tx.clone();
        let selector_clone = selector.clone();
        tokio::spawn(async move {
            // ns_owned only used to keep the param shape compatible with the
            // real informer — the in-memory client filters globally.
            let _ = ns_owned;
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        if let Some(sel) = &selector_clone {
                            if !sel.matches(ev.object.labels()) {
                                continue;
                            }
                        }
                        let kind = InformerEvent::from(ev.event);
                        match kind {
                            InformerEvent::Delete => {
                                store_clone.remove(&meta_namespace_key(&ev.object));
                            }
                            _ => {
                                store_clone.insert(ev.object.clone());
                            }
                        }
                        if tx_clone.send((kind, ev.object)).is_err() {
                            // No receivers — keep updating the store anyway,
                            // controllers may subscribe later.
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        Ok(Self {
            store,
            bus: tx,
            _phantom: PhantomData,
        })
    }

    /// Subscribe a new event handler. The receiver fires on every Add /
    /// Update / Delete event for the lifetime of the informer.
    pub fn subscribe(&self) -> broadcast::Receiver<(InformerEvent, R)> {
        self.bus.subscribe()
    }

    /// Borrow the underlying store.
    #[must_use]
    pub fn store(&self) -> Arc<Store<R>> {
        Arc::clone(&self.store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::{ControllerApiClient, InMemoryApiClient};
    use crate::types::{ObjectMeta, Pod};

    fn pod(ns: &str, name: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: name.into(),
                namespace: ns.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn meta_namespace_key_round_trip() {
        let p = pod("default", "nginx");
        let k = meta_namespace_key(&p);
        assert_eq!(k, "default/nginx");
        let (ns, name) = split_meta_namespace_key(&k);
        assert_eq!(ns, "default");
        assert_eq!(name, "nginx");
    }

    #[test]
    fn meta_namespace_key_cluster_scoped() {
        let mut p = pod("", "nginx");
        p.metadata.namespace = String::new();
        let k = meta_namespace_key(&p);
        assert_eq!(k, "nginx");
    }

    #[tokio::test]
    async fn informer_seeds_store_from_list() {
        let c = InMemoryApiClient::new();
        c.create::<Pod>(Some("default"), pod("default", "p1")).await.unwrap();
        c.create::<Pod>(Some("default"), pod("default", "p2")).await.unwrap();
        let inf = SharedInformer::<Pod>::start(c, Some("default")).await.unwrap();
        assert_eq!(inf.store().len(), 2);
    }

    #[tokio::test]
    async fn informer_observes_subsequent_writes() {
        let c = InMemoryApiClient::new();
        let inf = SharedInformer::<Pod>::start(c.clone(), Some("default"))
            .await
            .unwrap();
        let mut rx = inf.subscribe();
        c.create::<Pod>(Some("default"), pod("default", "p1")).await.unwrap();
        let (ev, _obj) = rx.recv().await.unwrap();
        assert_eq!(ev, InformerEvent::Add);
        assert_eq!(inf.store().len(), 1);
    }

    #[tokio::test]
    async fn informer_drops_deleted_from_store() {
        let c = InMemoryApiClient::new();
        c.create::<Pod>(Some("default"), pod("default", "p1")).await.unwrap();
        let inf = SharedInformer::<Pod>::start(c.clone(), Some("default"))
            .await
            .unwrap();
        let mut rx = inf.subscribe();
        c.delete("Pod", Some("default"), "p1").await.unwrap();
        let (ev, _obj) = rx.recv().await.unwrap();
        assert_eq!(ev, InformerEvent::Delete);
        assert_eq!(inf.store().len(), 0);
    }
}
