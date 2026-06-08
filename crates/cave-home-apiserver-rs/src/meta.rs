// SPDX-License-Identifier: Apache-2.0
//! Object metadata and the optimistic-concurrency model.
//!
//! Behavioural reference: Kubernetes API conventions (`api-conventions.md`,
//! "Metadata", "Concurrency Control and Consistency") and the documented
//! `ObjectMeta` semantics — `resourceVersion` optimistic concurrency,
//! `generation` bumps on spec change, `deletionTimestamp` + `finalizers`
//! foreground deletion, and `ownerReferences`. Clean-room reimplementation of
//! the documented contract.

use std::collections::BTreeMap;

use crate::json::Value;

/// A reference to an owning object (`metadata.ownerReferences[*]`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerReference {
    /// `apiVersion` of the owner.
    pub api_version: String,
    /// Owner kind.
    pub kind: String,
    /// Owner name.
    pub name: String,
    /// Owner UID.
    pub uid: String,
    /// If true, the owner is a controller (at most one per object).
    pub controller: bool,
    /// If true, the owner blocks deletion until this dependent is removed.
    pub block_owner_deletion: bool,
}

/// The metadata every API object carries that the decision core reasons about.
///
/// `resource_version` is an opaque monotonically-increasing token in real
/// Kubernetes; here it is the registry's global counter rendered as a decimal
/// string (an honest, documented modelling choice — see `parity.manifest.toml`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectMeta {
    /// Object name (unique within namespace + kind).
    pub name: String,
    /// Namespace; empty for cluster-scoped objects.
    pub namespace: String,
    /// Server-assigned UID.
    pub uid: String,
    /// Opaque optimistic-concurrency token.
    pub resource_version: String,
    /// Bumped by the server whenever `.spec` changes.
    pub generation: i64,
    /// Set (RFC3339-ish marker) when a delete has been requested but
    /// finalizers remain.
    pub deletion_timestamp: Option<String>,
    /// Finalizer keys that block actual deletion while present.
    pub finalizers: Vec<String>,
    /// Owning objects.
    pub owner_references: Vec<OwnerReference>,
    /// Label map (drives label selectors).
    pub labels: BTreeMap<String, String>,
    /// Annotation map.
    pub annotations: BTreeMap<String, String>,
}

impl ObjectMeta {
    /// True if a deletion has been requested (deletionTimestamp set).
    #[must_use]
    pub fn is_being_deleted(&self) -> bool {
        self.deletion_timestamp.is_some()
    }

    /// True if any finalizer is present.
    #[must_use]
    pub fn has_finalizers(&self) -> bool {
        !self.finalizers.is_empty()
    }

    /// The controller owner reference, if exactly one is marked `controller`.
    #[must_use]
    pub fn controller_owner(&self) -> Option<&OwnerReference> {
        self.owner_references.iter().find(|o| o.controller)
    }
}

/// Read `ObjectMeta` out of an object value tree (`metadata` field). Missing
/// fields default; this never panics.
#[must_use]
pub fn read_meta(object: &Value) -> ObjectMeta {
    let m = object.get("metadata");
    let s = |k: &str| -> String {
        m.and_then(|m| m.get(k)).and_then(Value::as_str).unwrap_or("").to_string()
    };
    let generation = m
        .and_then(|m| m.get("generation"))
        .and_then(|v| match v {
            Value::Number(n) => Some(*n as i64),
            _ => None,
        })
        .unwrap_or(0);
    let finalizers = m
        .and_then(|m| m.get("finalizers"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let deletion_timestamp = m
        .and_then(|m| m.get("deletionTimestamp"))
        .and_then(|v| v.as_str().map(str::to_string));
    let labels = read_string_map(m, "labels");
    let annotations = read_string_map(m, "annotations");
    let owner_references = m
        .and_then(|m| m.get("ownerReferences"))
        .and_then(Value::as_array)
        .map(|a| a.iter().map(read_owner_ref).collect())
        .unwrap_or_default();

    ObjectMeta {
        name: s("name"),
        namespace: s("namespace"),
        uid: s("uid"),
        resource_version: s("resourceVersion"),
        generation,
        deletion_timestamp,
        finalizers,
        owner_references,
        labels,
        annotations,
    }
}

fn read_string_map(m: Option<&Value>, key: &str) -> BTreeMap<String, String> {
    m.and_then(|m| m.get(key))
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn read_owner_ref(v: &Value) -> OwnerReference {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    OwnerReference {
        api_version: s("apiVersion"),
        kind: s("kind"),
        name: s("name"),
        uid: s("uid"),
        controller: v.get("controller").and_then(Value::as_bool).unwrap_or(false),
        block_owner_deletion: v
            .get("blockOwnerDeletion")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// Write the metadata fields the server controls back into an object value
/// tree, creating `metadata` if absent. Only server-managed fields are touched
/// (resourceVersion, generation, uid, deletionTimestamp, finalizers).
pub fn write_meta(object: &mut Value, meta: &ObjectMeta) {
    if object.as_object().is_none() {
        *object = Value::object();
    }
    let root = match object.as_object_mut() {
        Some(m) => m,
        None => return,
    };
    let md = root.entry("metadata".to_string()).or_insert_with(Value::object);
    if md.as_object().is_none() {
        *md = Value::object();
    }
    if let Some(m) = md.as_object_mut() {
        m.insert("name".into(), Value::from(meta.name.clone()));
        if !meta.namespace.is_empty() {
            m.insert("namespace".into(), Value::from(meta.namespace.clone()));
        }
        if !meta.uid.is_empty() {
            m.insert("uid".into(), Value::from(meta.uid.clone()));
        }
        m.insert("resourceVersion".into(), Value::from(meta.resource_version.clone()));
        m.insert("generation".into(), Value::from(meta.generation));
        match &meta.deletion_timestamp {
            Some(ts) => {
                m.insert("deletionTimestamp".into(), Value::from(ts.clone()));
            }
            None => {
                m.remove("deletionTimestamp");
            }
        }
        if meta.finalizers.is_empty() {
            m.remove("finalizers");
        } else {
            m.insert(
                "finalizers".into(),
                Value::Array(meta.finalizers.iter().cloned().map(Value::from).collect()),
            );
        }
    }
}

/// Extract the `.spec` subtree for comparison (None if absent).
#[must_use]
pub fn spec_of(object: &Value) -> Option<&Value> {
    object.get("spec")
}

/// Decide whether a generation bump is required: the spec changed between the
/// old and new object. Per the documented contract, `.metadata.generation` is
/// bumped only on spec mutations (not on status/metadata-only updates).
#[must_use]
pub fn spec_changed(old: &Value, new: &Value) -> bool {
    spec_of(old) != spec_of(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::obj;

    fn pod_with(meta: Value, spec: Value) -> Value {
        obj([("metadata", meta), ("spec", spec)])
    }

    #[test]
    fn read_meta_defaults_are_safe() {
        let m = read_meta(&Value::Null);
        assert_eq!(m.name, "");
        assert_eq!(m.generation, 0);
        assert!(!m.has_finalizers());
        assert!(!m.is_being_deleted());
    }

    #[test]
    fn read_meta_parses_fields() {
        let object = obj([(
            "metadata",
            obj([
                ("name", Value::from("nginx")),
                ("namespace", Value::from("default")),
                ("generation", Value::from(3_i64)),
                ("finalizers", Value::Array(vec![Value::from("kubernetes")])),
                ("deletionTimestamp", Value::from("2026-01-01T00:00:00Z")),
            ]),
        )]);
        let m = read_meta(&object);
        assert_eq!(m.name, "nginx");
        assert_eq!(m.generation, 3);
        assert!(m.has_finalizers());
        assert!(m.is_being_deleted());
    }

    #[test]
    fn write_then_read_round_trips_managed_fields() {
        let mut object = obj([("metadata", obj([("name", Value::from("p"))]))]);
        let meta = ObjectMeta {
            name: "p".into(),
            resource_version: "7".into(),
            generation: 2,
            finalizers: vec!["foo/bar".into()],
            ..ObjectMeta::default()
        };
        write_meta(&mut object, &meta);
        let back = read_meta(&object);
        assert_eq!(back.resource_version, "7");
        assert_eq!(back.generation, 2);
        assert_eq!(back.finalizers, vec!["foo/bar".to_string()]);
    }

    #[test]
    fn spec_changed_detects_spec_mutation() {
        let a = pod_with(Value::object(), obj([("replicas", Value::from(1_i64))]));
        let b = pod_with(Value::object(), obj([("replicas", Value::from(2_i64))]));
        assert!(spec_changed(&a, &b));
    }

    #[test]
    fn spec_unchanged_when_only_status_differs() {
        let a = obj([
            ("spec", obj([("replicas", Value::from(1_i64))])),
            ("status", obj([("ready", Value::from(true))])),
        ]);
        let b = obj([
            ("spec", obj([("replicas", Value::from(1_i64))])),
            ("status", obj([("ready", Value::from(false))])),
        ]);
        assert!(!spec_changed(&a, &b));
    }

    #[test]
    fn controller_owner_found() {
        let object = obj([(
            "metadata",
            obj([(
                "ownerReferences",
                Value::Array(vec![obj([
                    ("kind", Value::from("ReplicaSet")),
                    ("name", Value::from("rs-1")),
                    ("controller", Value::from(true)),
                ])]),
            )]),
        )]);
        let m = read_meta(&object);
        assert_eq!(m.owner_references.len(), 1);
        assert_eq!(m.controller_owner().map(|o| o.name.as_str()), Some("rs-1"));
    }
}
