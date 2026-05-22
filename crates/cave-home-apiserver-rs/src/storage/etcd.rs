// SPDX-License-Identifier: Apache-2.0
//! `EtcdStoragePlaceholder` — Phase 3 will wire the real etcd-over-SQLite
//! (`cave-home-kine-rs`). For Phase 2 this delegates every call to the
//! in-memory backend so downstream code can already select "etcd" by name
//! without any `unimplemented!()` calls.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! staging/src/k8s.io/apiserver/pkg/storage/etcd3/store.go::store

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::api::{ApiObject, WatchEvent};
use crate::types::ResourceRef;

use super::memory::InMemoryStorage;
use super::{Storage, StorageResult};

/// Stand-in for the etcd backend. Holds an `InMemoryStorage` for the actual
/// data so REST traffic works in dev mode; the real etcd client arrives in
/// Phase 3 via `cave-home-kine-rs`.
#[derive(Clone, Default)]
pub struct EtcdStoragePlaceholder {
    inner: InMemoryStorage,
    /// Endpoints that *would* be used once etcd is wired.
    endpoints: Vec<String>,
}

impl EtcdStoragePlaceholder {
    /// Construct a placeholder, recording the endpoints we'll dial in Phase 3.
    #[must_use]
    pub fn new(endpoints: Vec<String>) -> Self {
        Self {
            inner: InMemoryStorage::new(),
            endpoints,
        }
    }

    /// Inspect the (would-be) etcd endpoints.
    #[must_use]
    pub fn endpoints(&self) -> &[String] {
        &self.endpoints
    }
}

#[async_trait]
impl Storage for EtcdStoragePlaceholder {
    async fn create(&self, key: &ResourceRef, obj: ApiObject) -> StorageResult<ApiObject> {
        self.inner.create(key, obj).await
    }

    async fn get(&self, key: &ResourceRef) -> StorageResult<ApiObject> {
        self.inner.get(key).await
    }

    async fn update(&self, key: &ResourceRef, obj: ApiObject) -> StorageResult<ApiObject> {
        self.inner.update(key, obj).await
    }

    async fn delete(&self, key: &ResourceRef) -> StorageResult<ApiObject> {
        self.inner.delete(key).await
    }

    async fn list(&self, key: &ResourceRef) -> StorageResult<Vec<ApiObject>> {
        self.inner.list(key).await
    }

    fn watch(&self, key: &ResourceRef) -> broadcast::Receiver<WatchEvent> {
        self.inner.watch(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn placeholder_records_endpoints() {
        let s = EtcdStoragePlaceholder::new(vec!["http://127.0.0.1:2379".to_string()]);
        assert_eq!(s.endpoints().len(), 1);
    }

    #[tokio::test]
    async fn placeholder_round_trips_through_memory() {
        let s = EtcdStoragePlaceholder::new(vec![]);
        let key = ResourceRef::namespaced("", "v1", "pods", "default", "p");
        let pod = ApiObject::new("v1", "Pod", "p").with_namespace("default");
        s.create(&key, pod).await.expect("create");
        let got = s.get(&key).await.expect("get");
        assert_eq!(got.metadata.name, "p");
    }
}
