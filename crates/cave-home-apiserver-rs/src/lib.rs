// SPDX-License-Identifier: Apache-2.0
//! cave-home-apiserver-rs — Kubernetes API server (control-plane).
//!
//! Line-by-line port of `staging/src/k8s.io/apiserver/` and the relevant
//! slices of `pkg/registry/` from `kubernetes/kubernetes` v1.36.1
//! (SHA `756939600b9a7180fc2df6550a4585b638875e67`).
//!
//! Phase 2 MVP scope (per ADR-004, ROADMAP M3):
//! - [`api`]            — k8s API type subset (core/v1, apps/v1, batch/v1).
//! - [`storage`]        — `Storage` trait + in-memory + etcd-placeholder.
//! - [`auth`]           — `Authenticator` (cert + SA token) + `Authorizer` (RBAC).
//! - [`admission`]      — chain executor + NamespaceLifecycle + ServiceAccount.
//! - [`serialization`]  — JSON / YAML codec.
//! - [`client_trait`]   — `ApiClient` trait re-exported for downstream crates.
//! - [`server`]         — HTTP layer (axum) wiring registry + auth + admission.
//!
//! Out of Phase 2 scope (see `parity.manifest.toml` `[[unmapped]]` entries):
//! webhook admission, CRDs, aggregated API, full OpenAPI v3, real etcd
//! (lands as `cave-home-kine-rs` integration in Phase 3).

pub mod admission;
pub mod api;
pub mod auth;
pub mod client_trait;
pub mod serialization;
pub mod server;
pub mod storage;
pub mod types;

pub use admission::{
    AdmissionChain, AdmissionController, AdmissionError, NamespaceLifecycle, ServiceAccount,
};
pub use auth::{Authenticator, Authorizer, AuthzDecision, RbacAuthorizer};
pub use client_trait::{ApiClient, ApiClientError, ApiResult};
pub use serialization::{decode as decode_object, encode as encode_object};
pub use server::{ApiServer, InProcessClient};
pub use storage::{Storage, StorageError, StorageResult};
pub use types::{ResourceRef, UserInfo};
