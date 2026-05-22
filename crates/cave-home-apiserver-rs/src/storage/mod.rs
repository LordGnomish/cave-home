// SPDX-License-Identifier: Apache-2.0
//! Persistent object store abstraction.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! - staging/src/k8s.io/apiserver/pkg/storage/interfaces.go::Interface
//! - staging/src/k8s.io/apiserver/pkg/registry/generic/registry/store.go::Store

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::api::{ApiObject, WatchEvent};
use crate::types::ResourceRef;

pub mod etcd;
pub mod memory;

pub use etcd::EtcdStoragePlaceholder;
pub use memory::InMemoryStorage;

/// Errors returned by every `Storage` method.
///
/// Source: staging/src/k8s.io/apiserver/pkg/storage/errors.go
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum StorageError {
    /// Key does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// Key already exists (CREATE collision).
    #[error("already exists: {0}")]
    AlreadyExists(String),
    /// `resourceVersion` mismatch on UPDATE.
    #[error("conflict on {0}: stored={stored} requested={requested}", stored = .1, requested = .2)]
    Conflict(String, String, String),
    /// Invalid input.
    #[error("invalid: {0}")]
    Invalid(String),
    /// Underlying I/O error.
    #[error("internal: {0}")]
    Internal(String),
}

/// Convenience result alias.
pub type StorageResult<T> = Result<T, StorageError>;

/// Persistent object store interface.
///
/// Source: staging/src/k8s.io/apiserver/pkg/storage/interfaces.go::Interface
#[async_trait]
pub trait Storage: Send + Sync {
    /// Insert a new object. Errors with `AlreadyExists` on collision.
    async fn create(&self, key: &ResourceRef, obj: ApiObject) -> StorageResult<ApiObject>;

    /// Fetch one object. Errors with `NotFound`.
    async fn get(&self, key: &ResourceRef) -> StorageResult<ApiObject>;

    /// Update an existing object. Errors with `NotFound` or `Conflict`.
    async fn update(&self, key: &ResourceRef, obj: ApiObject) -> StorageResult<ApiObject>;

    /// Delete one object. Returns the prior value. Errors with `NotFound`.
    async fn delete(&self, key: &ResourceRef) -> StorageResult<ApiObject>;

    /// List all objects matching `(group, version, resource[, namespace])`.
    /// `key.name` is ignored.
    async fn list(&self, key: &ResourceRef) -> StorageResult<Vec<ApiObject>>;

    /// Subscribe to a broadcast channel of `WatchEvent`s for the given
    /// `(group, version, resource[, namespace])` scope.
    fn watch(&self, key: &ResourceRef) -> broadcast::Receiver<WatchEvent>;
}
