// SPDX-License-Identifier: Apache-2.0
//! Kubernetes API type subset.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! - staging/src/k8s.io/api/core/v1/types.go
//! - staging/src/k8s.io/api/apps/v1/types.go
//! - staging/src/k8s.io/api/batch/v1/types.go
//!
//! Phase 2 deliberately models only the fields actually exercised by the
//! REST surface: metadata + a coarse `spec` / `status` blob carried as
//! `serde_json::Value`. Strongly-typed full schemas are Phase 2b.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod apps_v1;
pub mod batch_v1;
pub mod core_v1;

/// Look up a `(group, version, resource)` triple across the known groups
/// and return `(kind, is_namespaced)` if recognised.
#[must_use]
pub fn resolve_kind(group: &str, version: &str, resource: &str) -> Option<(&'static str, bool)> {
    match (group, version) {
        ("", "v1") => core_v1::KINDS
            .iter()
            .find(|(r, _, _)| *r == resource)
            .map(|(_, k, ns)| (*k, *ns)),
        ("apps", "v1") => apps_v1::KINDS
            .iter()
            .find(|(r, _, _)| *r == resource)
            .map(|(_, k, ns)| (*k, *ns)),
        ("batch", "v1") => batch_v1::KINDS
            .iter()
            .find(|(r, _, _)| *r == resource)
            .map(|(_, k, ns)| (*k, *ns)),
        _ => None,
    }
}

/// `ObjectMeta` subset that every resource needs for REST routing.
///
/// Source: staging/src/k8s.io/apimachinery/pkg/apis/meta/v1/types.go::ObjectMeta
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectMeta {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub namespace: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    #[serde(default, rename = "resourceVersion", skip_serializing_if = "String::is_empty")]
    pub resource_version: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
    #[serde(
        default,
        rename = "creationTimestamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub creation_timestamp: Option<String>,
    #[serde(default, rename = "deletionTimestamp", skip_serializing_if = "Option::is_none")]
    pub deletion_timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finalizers: Vec<String>,
}

/// Every API object carried by REST shares these top-level fields.
///
/// Source: staging/src/k8s.io/apimachinery/pkg/runtime/types.go::TypeMeta
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TypeMeta {
    #[serde(default, rename = "apiVersion", skip_serializing_if = "String::is_empty")]
    pub api_version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
}

/// Generic object envelope used as the canonical wire form by storage.
///
/// In real upstream Kubernetes each kind has its own Go struct; we keep one
/// envelope with `spec` / `status` as `serde_json::Value` so the registry can
/// store every kind uniformly without per-kind generics. Strongly-typed
/// per-kind structs live in `core_v1`, `apps_v1`, `batch_v1` for the kinds
/// that have non-trivial validation in Phase 2.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApiObject {
    #[serde(flatten)]
    pub type_meta: TypeMeta,
    pub metadata: ObjectMeta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<serde_json::Value>,
    /// Additional top-level fields preserved verbatim (Endpoints' `subsets`,
    /// Secrets' `data`, ConfigMaps' `data`, etc).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl ApiObject {
    /// Construct a minimal object for tests.
    #[must_use]
    pub fn new(api_version: &str, kind: &str, name: &str) -> Self {
        Self {
            type_meta: TypeMeta {
                api_version: api_version.to_string(),
                kind: kind.to_string(),
            },
            metadata: ObjectMeta {
                name: name.to_string(),
                ..ObjectMeta::default()
            },
            spec: None,
            status: None,
            extra: BTreeMap::new(),
        }
    }

    /// Set namespace fluent-style.
    #[must_use]
    pub fn with_namespace(mut self, ns: &str) -> Self {
        self.metadata.namespace = ns.to_string();
        self
    }
}

/// List envelope returned by LIST.
///
/// Source: staging/src/k8s.io/apimachinery/pkg/apis/meta/v1/types.go::List
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ApiList {
    #[serde(flatten)]
    pub type_meta: TypeMeta,
    pub metadata: ListMeta,
    pub items: Vec<ApiObject>,
}

/// `ListMeta` for collection responses.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ListMeta {
    #[serde(default, rename = "resourceVersion", skip_serializing_if = "String::is_empty")]
    pub resource_version: String,
    #[serde(default, rename = "continue", skip_serializing_if = "String::is_empty")]
    pub continue_token: String,
}

/// Watch event wire format.
///
/// Source: staging/src/k8s.io/apimachinery/pkg/watch/watch.go::Event
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WatchEvent {
    #[serde(rename = "type")]
    pub event_type: WatchEventType,
    pub object: ApiObject,
}

/// Watch event kind.
///
/// Source: staging/src/k8s.io/apimachinery/pkg/watch/watch.go::EventType
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WatchEventType {
    #[serde(rename = "ADDED")]
    Added,
    #[serde(rename = "MODIFIED")]
    Modified,
    #[serde(rename = "DELETED")]
    Deleted,
    #[serde(rename = "BOOKMARK")]
    Bookmark,
    #[serde(rename = "ERROR")]
    Error,
}

/// `Status` envelope used for errors / DELETE responses.
///
/// Source: staging/src/k8s.io/apimachinery/pkg/apis/meta/v1/types.go::Status
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Status {
    #[serde(flatten)]
    pub type_meta: TypeMeta,
    pub metadata: ListMeta,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    pub code: u16,
}

impl Status {
    /// Construct a success Status (used by DELETE).
    #[must_use]
    pub fn success() -> Self {
        Self {
            type_meta: TypeMeta {
                api_version: "v1".into(),
                kind: "Status".into(),
            },
            metadata: ListMeta::default(),
            status: "Success".to_string(),
            message: String::new(),
            reason: String::new(),
            code: 200,
        }
    }

    /// Construct a failure Status.
    #[must_use]
    pub fn failure(code: u16, reason: &str, message: &str) -> Self {
        Self {
            type_meta: TypeMeta {
                api_version: "v1".into(),
                kind: "Status".into(),
            },
            metadata: ListMeta::default(),
            status: "Failure".to_string(),
            message: message.to_string(),
            reason: reason.to_string(),
            code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_object_round_trips_through_json() {
        let pod = ApiObject::new("v1", "Pod", "nginx").with_namespace("default");
        let s = serde_json::to_string(&pod).expect("serialize");
        let back: ApiObject = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(back, pod);
    }

    #[test]
    fn unknown_top_level_field_is_preserved() {
        let raw = serde_json::json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {"name": "cm", "namespace": "default"},
            "data": {"foo": "bar"}
        });
        let obj: ApiObject = serde_json::from_value(raw).expect("deserialize");
        assert!(obj.extra.contains_key("data"));
        let back = serde_json::to_value(&obj).expect("serialize");
        assert_eq!(back["data"]["foo"], "bar");
    }

    #[test]
    fn status_success_has_code_200() {
        let s = Status::success();
        assert_eq!(s.code, 200);
        assert_eq!(s.status, "Success");
    }

    #[test]
    fn watch_event_serializes_uppercase() {
        let evt = WatchEvent {
            event_type: WatchEventType::Added,
            object: ApiObject::new("v1", "Pod", "p"),
        };
        let s = serde_json::to_string(&evt).expect("serialize");
        assert!(s.contains("\"ADDED\""));
    }
}
