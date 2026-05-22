// SPDX-License-Identifier: Apache-2.0
//! Stable `ApiClient` trait — RED phase scaffold.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! staging/src/k8s.io/client-go/rest/client.go::Interface
//!
//! This is the surface that the scheduler / controller-manager / kubelet
//! crates import. Mirrors the role of `CriClient` in cave-home-kubelet-rs:
//! exposed publicly so callers can depend on the contract without pulling
//! in apiserver internals.

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::api::{ApiObject, WatchEvent};
use crate::types::ResourceRef;

/// Errors returned by every `ApiClient` method.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ApiClientError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// Convenience alias.
pub type ApiResult<T> = Result<T, ApiClientError>;

/// Stable client surface used by other cave-home crates.
///
/// Mirrors `staging/src/k8s.io/client-go/rest/client.go::Interface`'s
/// CRUD verbs while staying transport-agnostic (the in-process impl
/// shipped alongside the apiserver dispatches directly to the registry;
/// an HTTP impl will land in Phase 2b for the kubelet).
#[async_trait]
pub trait ApiClient: Send + Sync {
    async fn get(&self, key: &ResourceRef) -> ApiResult<ApiObject>;
    async fn list(&self, key: &ResourceRef) -> ApiResult<Vec<ApiObject>>;
    /// Subscribe to events for the given resource scope.
    fn watch(&self, key: &ResourceRef) -> broadcast::Receiver<WatchEvent>;
    async fn create(&self, key: &ResourceRef, obj: ApiObject) -> ApiResult<ApiObject>;
    async fn update(&self, key: &ResourceRef, obj: ApiObject) -> ApiResult<ApiObject>;
    /// JSON-Merge-Patch (RFC 7396) — Phase 2 supports this style only;
    /// JSON-Patch (RFC 6902) and strategic-merge-patch arrive in Phase 2b.
    async fn patch(&self, key: &ResourceRef, patch: serde_json::Value) -> ApiResult<ApiObject>;
    async fn delete(&self, key: &ResourceRef) -> ApiResult<ApiObject>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: a trait-object can be constructed and stored.
    #[test]
    fn trait_object_is_sized() {
        // The trait is dyn-compatible — verified at compile time below.
        fn assert_obj_safe<T: ?Sized>() {}
        assert_obj_safe::<dyn ApiClient>();
    }
}
