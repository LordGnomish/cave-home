// SPDX-License-Identifier: Apache-2.0
//! Minimal, vendor-neutral object model shared by the controllers.
//!
//! This is a deliberately small subset of the Kubernetes apimachinery
//! `ObjectMeta` / `OwnerReference` shape — only the fields the pure decision
//! logic in this crate actually reads. The full typed API surface (versioned
//! `apps/v1`, `batch/v1`, etc.) and serde wire formats are deferred to the
//! apiserver-client phase (see `parity.manifest.toml`).
//!
//! Everything here is plain `std`: `String`, `BTreeMap`, no async, no I/O.

use std::collections::BTreeMap;

/// A stable, cluster-unique identity for an object (apimachinery `UID`).
///
/// Controllers reason about ownership and identity over `Uid`, never over the
/// mutable `(namespace, name)` pair, mirroring the apiserver contract that a
/// name can be reused after deletion but a UID never is.
pub type Uid = String;

/// A reference from a dependent object to one of its owners.
///
/// Behavioural subset of apimachinery `OwnerReference`. The garbage collector
/// reads [`OwnerReference::uid`], [`OwnerReference::controller`] and
/// [`OwnerReference::block_owner_deletion`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerReference {
    /// `kind` of the owner (e.g. `"ReplicaSet"`).
    pub kind: String,
    /// `name` of the owner.
    pub name: String,
    /// `uid` of the owner — the field ownership is actually keyed on.
    pub uid: Uid,
    /// Whether the owner is the managing controller of this object.
    pub controller: bool,
    /// Whether foreground deletion of this dependent must block the owner's
    /// deletion until the dependent is gone.
    pub block_owner_deletion: bool,
}

impl OwnerReference {
    /// A non-controller, non-blocking owner reference to `uid`.
    #[must_use]
    pub fn to(kind: &str, name: &str, uid: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            name: name.to_owned(),
            uid: uid.to_owned(),
            controller: false,
            block_owner_deletion: false,
        }
    }

    /// Mark this reference as the managing controller.
    #[must_use]
    pub const fn controller(mut self) -> Self {
        self.controller = true;
        self
    }

    /// Mark this reference as blocking foreground owner deletion.
    #[must_use]
    pub const fn blocking(mut self) -> Self {
        self.block_owner_deletion = true;
        self
    }
}

/// The metadata every object carries (apimachinery `ObjectMeta` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjectMeta {
    /// Object name, unique within a namespace.
    pub name: String,
    /// Namespace, empty for cluster-scoped objects.
    pub namespace: String,
    /// Stable identity. Empty means "not yet persisted" in this model.
    pub uid: Uid,
    /// Key/value labels used by selector-based indexing.
    pub labels: BTreeMap<String, String>,
    /// Active finalizers. While non-empty, an object with a deletion timestamp
    /// is retained (apimachinery finalizer contract).
    pub finalizers: Vec<String>,
    /// Owners of this object.
    pub owner_references: Vec<OwnerReference>,
    /// Logical deletion time, in caller-supplied epoch seconds. `Some` means
    /// the object is in the "terminating" state.
    pub deletion_timestamp: Option<i64>,
}

impl ObjectMeta {
    /// Construct metadata with a name, namespace and UID.
    #[must_use]
    pub fn new(name: &str, namespace: &str, uid: &str) -> Self {
        Self {
            name: name.to_owned(),
            namespace: namespace.to_owned(),
            uid: uid.to_owned(),
            ..Self::default()
        }
    }

    /// Attach a label, builder-style.
    #[must_use]
    pub fn with_label(mut self, key: &str, value: &str) -> Self {
        self.labels.insert(key.to_owned(), value.to_owned());
        self
    }

    /// Attach an owner reference, builder-style.
    #[must_use]
    pub fn with_owner(mut self, owner: OwnerReference) -> Self {
        self.owner_references.push(owner);
        self
    }

    /// Attach a finalizer, builder-style.
    #[must_use]
    pub fn with_finalizer(mut self, finalizer: &str) -> Self {
        self.finalizers.push(finalizer.to_owned());
        self
    }

    /// `true` if this object has a deletion timestamp set.
    #[must_use]
    pub const fn is_terminating(&self) -> bool {
        self.deletion_timestamp.is_some()
    }
}

/// Anything stored in the indexer / reconciled by a controller.
///
/// A trait rather than a concrete enum so the framework code (`Store`,
/// `DeltaFifo`, `Reconciler`) stays generic over object kind without any
/// dependency on a particular typed API.
pub trait Object: Clone {
    /// Borrow this object's metadata.
    fn meta(&self) -> &ObjectMeta;

    /// The namespace/name key used by the store (`"<ns>/<name>"`, or just
    /// `"<name>"` for cluster-scoped objects).
    fn key(&self) -> String {
        let m = self.meta();
        if m.namespace.is_empty() {
            m.name.clone()
        } else {
            format!("{}/{}", m.namespace, m.name)
        }
    }
}

impl Object for ObjectMeta {
    fn meta(&self) -> &ObjectMeta {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_namespaced_for_namespaced_objects() {
        let m = ObjectMeta::new("web", "prod", "u1");
        assert_eq!(m.key(), "prod/web");
    }

    #[test]
    fn key_is_bare_name_for_cluster_scoped_objects() {
        let m = ObjectMeta::new("node-a", "", "u2");
        assert_eq!(m.key(), "node-a");
    }

    #[test]
    fn owner_reference_builders_set_flags() {
        let owner = OwnerReference::to("ReplicaSet", "rs", "rsuid")
            .controller()
            .blocking();
        assert!(owner.controller);
        assert!(owner.block_owner_deletion);
        assert_eq!(owner.uid, "rsuid");
    }

    #[test]
    fn terminating_reflects_deletion_timestamp() {
        let mut m = ObjectMeta::new("x", "ns", "u");
        assert!(!m.is_terminating());
        m.deletion_timestamp = Some(100);
        assert!(m.is_terminating());
    }

    #[test]
    fn label_and_finalizer_builders_accumulate() {
        let m = ObjectMeta::new("x", "ns", "u")
            .with_label("app", "web")
            .with_finalizer("kubernetes");
        assert_eq!(m.labels.get("app").map(String::as_str), Some("web"));
        assert_eq!(m.finalizers, vec!["kubernetes".to_owned()]);
    }
}
