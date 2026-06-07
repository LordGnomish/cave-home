// SPDX-License-Identifier: Apache-2.0
//! cave-home-apiserver-rs — the Kubernetes API-server *decision core*.
//!
//! INFRASTRUCTURE (ADR-004 orchestration layer). This crate is **hidden from
//! end users** (Charter §6.3): it produces no user-facing strings, carries no
//! i18n, and has no Portal/mobile UI. Correctness is the only product.
//!
//! ## What this is
//!
//! A std-only, dependency-free reimplementation of the REST resource-handling
//! semantics of a Kubernetes API server — the *brain* that decides what a verb
//! does, not the wire plumbing:
//!
//! - [`gvk`]      — `GroupVersionKind` / `GroupVersionResource` + RESTMapper.
//! - [`path`]     — the REST URL grammar (parse + build, namespaced/cluster).
//! - [`meta`]     — `ObjectMeta`: resourceVersion, generation, finalizers,
//!   `deletionTimestamp`, ownerReferences.
//! - [`selector`] — label + field selector parsing and matching.
//! - [`patch`]    — JSON Merge Patch (RFC 7396) + JSON Patch (RFC 6902).
//! - [`registry`] — the in-memory store with full verb semantics: create
//!   (AlreadyExists), get (NotFound), list (selectors + pagination),
//!   update/patch (optimistic concurrency → Conflict), finalizer-aware delete,
//!   watch replay.
//! - [`admission`]— two-phase (mutating → validating) admission pipeline with a
//!   couple of built-in rules.
//! - [`rbac`]     — the additive RBAC authorizer: Role/ClusterRole +
//!   RoleBinding/ClusterRoleBinding rule matching → Allow / NoOpinion.
//! - [`discovery`]— the discovery surface: served groups/versions/resources
//!   (`APIVersions` / `APIGroupList` / `APIResourceList`) over the kind table.
//! - [`status`]   — the `metav1.Status` error model (code / reason / message).
//! - [`json`]     — a small std-only JSON value tree the above operate on.
//!
//! ## Honest port method
//!
//! This is a **behavioural reimplementation of the documented Kubernetes REST
//! semantics** (the public API conventions, label/field selector docs, RFC
//! 7396 / RFC 6902, the admission-controller phase contract). It is **not** a
//! verbatim line-by-line transcription of any pinned `kubernetes/kubernetes`
//! revision. The additive RBAC authorizer is implemented here; the HTTP/2
//! server, etcd/kine storage backend, TLS + authentication, the Node authorizer
//! / RBAC aggregation / SubjectAccessReview surfaces, admission webhooks, CRDs,
//! and API aggregation are deferred and enumerated in `parity.manifest.toml`.
//!
//! ## Example
//!
//! ```
//! use cave_home_apiserver_rs::gvk::GroupVersionResource;
//! use cave_home_apiserver_rs::json::{obj, Value};
//! use cave_home_apiserver_rs::registry::Registry;
//! use cave_home_apiserver_rs::status::StatusReason;
//!
//! let pods = GroupVersionResource::new("", "v1", "pods");
//! let mut reg = Registry::new();
//!
//! let pod = obj([(
//!     "metadata",
//!     obj([("name", Value::from("nginx")), ("namespace", Value::from("default"))]),
//! )]);
//! let created = reg.create(&pods, pod).expect("create");
//! assert_eq!(created.pointer("metadata.resourceVersion"), Some(&Value::from("1")));
//!
//! // A second create with the same name is rejected AlreadyExists (409).
//! let dup = obj([(
//!     "metadata",
//!     obj([("name", Value::from("nginx")), ("namespace", Value::from("default"))]),
//! )]);
//! let err = reg.create(&pods, dup).unwrap_err();
//! assert_eq!(err.reason, StatusReason::AlreadyExists);
//! assert_eq!(err.code, 409);
//! ```

pub mod admission;
pub mod discovery;
pub mod gvk;
pub mod http;
pub mod json;
pub mod meta;
pub mod patch;
pub mod path;
pub mod rbac;
pub mod registry;
pub mod selector;
pub mod status;

pub use admission::{
    AdmissionChain, AdmissionRequest, DefaultFields, MutatingPlugin, NamespaceExists, Operation,
    RequireName, ValidatingPlugin,
};
pub use discovery::{ApiGroup, ApiResource, GroupVersionEntry};
pub use gvk::{GroupVersionKind, GroupVersionResource, RegisteredKind};
pub use json::Value;
pub use meta::{ObjectMeta, OwnerReference};
pub use patch::PatchOp;
pub use path::{parse as parse_path, ResourcePath};
pub use rbac::{
    Attributes, ClusterRole, ClusterRoleBinding, Decision, PolicyRule, RbacAuthorizer, Role,
    RoleBinding, RoleRef, RoleRefKind, Subject, SubjectKind, UserInfo,
};
pub use registry::{ListOptions, ListResult, Registry, WatchEvent, WatchEventKind};
pub use selector::{FieldSelector, LabelSelector, Requirement};
pub use status::{Status, StatusReason};
