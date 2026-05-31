// SPDX-License-Identifier: Apache-2.0
//! In-memory REST registry: the verb semantics over a per-resource store.
//!
//! Behavioural reference: Kubernetes API conventions (`api-conventions.md`,
//! verbs: get/list/create/update/patch/delete/watch; "Concurrency Control";
//! "Resource Expiration" / list pagination with `limit` + `continue`). This is
//! a clean-room reimplementation of the documented REST verb contract over an
//! in-memory store — the actual etcd/kine storage backend and the HTTP
//! transport are deferred (see `parity.manifest.toml`).

use std::collections::BTreeMap;

use crate::gvk::{self, GroupVersionResource};
use crate::json::Value;
use crate::meta::{self, ObjectMeta};
use crate::patch::{self, PatchOp};
use crate::selector::{FieldSelector, LabelSelector};
use crate::status::{Result, Status, StatusReason};

/// A watch event emitted after a mutation, replayed to watchers tracking a
/// resourceVersion.
#[derive(Clone, Debug, PartialEq)]
pub struct WatchEvent {
    /// Event kind.
    pub kind: WatchEventKind,
    /// The object at the time of the event.
    pub object: Value,
    /// The resourceVersion at which the event occurred.
    pub resource_version: u64,
}

/// The kind of a watch event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchEventKind {
    /// Object created.
    Added,
    /// Object updated/patched.
    Modified,
    /// Object deleted.
    Deleted,
}

/// Options controlling a list call.
#[derive(Clone, Debug, Default)]
pub struct ListOptions {
    /// Restrict to a namespace; `None` lists across all namespaces.
    pub namespace: Option<String>,
    /// Label selector (everything if default).
    pub label_selector: LabelSelector,
    /// Field selector (everything if default).
    pub field_selector: FieldSelector,
    /// Page size; 0 = unbounded.
    pub limit: usize,
    /// Opaque continue token from a previous page.
    pub continue_token: Option<String>,
}

/// A paginated list result.
#[derive(Clone, Debug, PartialEq)]
pub struct ListResult {
    /// The objects on this page (sorted by namespace then name).
    pub items: Vec<Value>,
    /// Continue token for the next page, if more remain.
    pub continue_token: Option<String>,
    /// The store's resourceVersion at list time.
    pub resource_version: u64,
}

/// A single namespaced+named key into the store.
fn store_key(namespace: &str, name: &str) -> String {
    format!("{namespace}/{name}")
}

/// The registry: holds every registered resource's objects plus the global
/// monotonic resourceVersion counter and a bounded watch-event history.
#[derive(Debug, Default)]
pub struct Registry {
    /// gvr -> (store_key -> object)
    stores: BTreeMap<GroupVersionResource, BTreeMap<String, Value>>,
    /// Monotonic resourceVersion counter (shared across all resources, as in
    /// upstream where it derives from a single etcd revision).
    rv: u64,
    /// Per-gvr ordered event history for watch replay.
    history: BTreeMap<GroupVersionResource, Vec<WatchEvent>>,
    /// UID counter for server-assigned UIDs.
    uid_seq: u64,
}

impl Registry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The current global resourceVersion.
    #[must_use]
    pub fn resource_version(&self) -> u64 {
        self.rv
    }

    fn next_rv(&mut self) -> u64 {
        self.rv += 1;
        self.rv
    }

    fn next_uid(&mut self) -> String {
        self.uid_seq += 1;
        format!("uid-{:08}", self.uid_seq)
    }

    fn check_known(gvr: &GroupVersionResource) -> Result<bool> {
        gvk::is_namespaced(gvr).ok_or_else(|| {
            Status::new(
                StatusReason::NotFound,
                format!("the server could not find the requested resource ({})", gvr.resource),
            )
        })
    }

    fn record(&mut self, gvr: &GroupVersionResource, kind: WatchEventKind, object: Value, rv: u64) {
        self.history.entry(gvr.clone()).or_default().push(WatchEvent {
            kind,
            object,
            resource_version: rv,
        });
    }

    /// CREATE: reject a duplicate name (`AlreadyExists` 409). Assigns uid,
    /// resourceVersion, generation=1, and emits an Added event.
    ///
    /// # Errors
    /// `NotFound` for unknown resource, `AlreadyExists` for a name collision.
    pub fn create(&mut self, gvr: &GroupVersionResource, mut object: Value) -> Result<Value> {
        let namespaced = Self::check_known(gvr)?;
        let mut m = meta::read_meta(&object);
        if m.name.is_empty() {
            return Err(Status::invalid("metadata.name is required"));
        }
        if namespaced && m.namespace.is_empty() {
            return Err(Status::invalid(format!(
                "namespace is required for {}",
                gvr.resource
            )));
        }
        let key = store_key(&m.namespace, &m.name);
        if self.stores.get(gvr).map(|s| s.contains_key(&key)).unwrap_or(false) {
            return Err(Status::already_exists(format!(
                "{} \"{}\" already exists",
                gvr.resource, m.name
            )));
        }
        let rv = self.next_rv();
        m.uid = self.next_uid();
        m.resource_version = rv.to_string();
        m.generation = 1;
        m.deletion_timestamp = None;
        meta::write_meta(&mut object, &m);
        self.stores
            .entry(gvr.clone())
            .or_default()
            .insert(key, object.clone());
        self.record(gvr, WatchEventKind::Added, object.clone(), rv);
        Ok(object)
    }

    /// GET a single object (`NotFound` 404 if absent).
    ///
    /// # Errors
    /// `NotFound` for unknown resource or missing object.
    pub fn get(&self, gvr: &GroupVersionResource, namespace: &str, name: &str) -> Result<Value> {
        Self::check_known(gvr)?;
        let key = store_key(namespace, name);
        self.stores
            .get(gvr)
            .and_then(|s| s.get(&key))
            .cloned()
            .ok_or_else(|| Status::not_found(format!("{} \"{name}\" not found", gvr.resource)))
    }

    /// UPDATE/replace with optimistic concurrency. A stale `resourceVersion`
    /// yields `Conflict` (409). Bumps generation when `.spec` changes.
    ///
    /// # Errors
    /// `NotFound`, `Conflict`, or `Invalid`.
    pub fn update(&mut self, gvr: &GroupVersionResource, mut object: Value) -> Result<Value> {
        Self::check_known(gvr)?;
        let incoming = meta::read_meta(&object);
        if incoming.name.is_empty() {
            return Err(Status::invalid("metadata.name is required"));
        }
        let key = store_key(&incoming.namespace, &incoming.name);
        let existing = self
            .stores
            .get(gvr)
            .and_then(|s| s.get(&key))
            .cloned()
            .ok_or_else(|| Status::not_found(format!("{} \"{}\" not found", gvr.resource, incoming.name)))?;
        let current = meta::read_meta(&existing);

        // Optimistic concurrency: the client must supply the current rv.
        if incoming.resource_version != current.resource_version {
            return Err(Status::conflict(format!(
                "Operation cannot be fulfilled on {} \"{}\": the object has been modified; please apply your changes to the latest version and try again",
                gvr.resource, incoming.name
            )));
        }

        let rv = self.next_rv();
        let mut new_meta = current.clone();
        new_meta.resource_version = rv.to_string();
        // Carry forward the new mutable metadata the client may have changed
        // (labels/annotations/finalizers), but keep server-owned identity.
        new_meta.finalizers.clone_from(&incoming.finalizers);
        // Generation bumps only on spec change.
        if meta::spec_changed(&existing, &object) {
            new_meta.generation = current.generation + 1;
        }
        meta::write_meta(&mut object, &new_meta);
        self.stores
            .entry(gvr.clone())
            .or_default()
            .insert(key, object.clone());
        self.record(gvr, WatchEventKind::Modified, object.clone(), rv);
        Ok(object)
    }

    /// PATCH: apply a merge or JSON patch to the stored object, then run it
    /// through the same concurrency + generation path as update (the patched
    /// object inherits the stored resourceVersion, so it never self-conflicts).
    ///
    /// # Errors
    /// `NotFound` or `Invalid`.
    pub fn patch_merge(
        &mut self,
        gvr: &GroupVersionResource,
        namespace: &str,
        name: &str,
        patch_doc: &Value,
    ) -> Result<Value> {
        let existing = self.get(gvr, namespace, name)?;
        let patched = patch::apply_merge_patch(&existing, patch_doc);
        self.apply_patched(gvr, namespace, name, existing, patched)
    }

    /// PATCH with an RFC 6902 JSON Patch op list.
    ///
    /// # Errors
    /// `NotFound` or `Invalid`.
    pub fn patch_json(
        &mut self,
        gvr: &GroupVersionResource,
        namespace: &str,
        name: &str,
        ops: &[PatchOp],
    ) -> Result<Value> {
        let existing = self.get(gvr, namespace, name)?;
        let patched = patch::apply_json_patch(&existing, ops)?;
        self.apply_patched(gvr, namespace, name, existing, patched)
    }

    fn apply_patched(
        &mut self,
        gvr: &GroupVersionResource,
        namespace: &str,
        name: &str,
        existing: Value,
        mut patched: Value,
    ) -> Result<Value> {
        let current = meta::read_meta(&existing);
        let rv = self.next_rv();
        let mut new_meta = meta::read_meta(&patched);
        new_meta.uid = current.uid.clone();
        new_meta.resource_version = rv.to_string();
        new_meta.generation = if meta::spec_changed(&existing, &patched) {
            current.generation + 1
        } else {
            current.generation
        };
        meta::write_meta(&mut patched, &new_meta);
        let key = store_key(namespace, name);
        self.stores
            .entry(gvr.clone())
            .or_default()
            .insert(key, patched.clone());
        self.record(gvr, WatchEventKind::Modified, patched.clone(), rv);
        Ok(patched)
    }

    /// DELETE: finalizer-aware. If finalizers remain, the object is *not*
    /// removed; instead `deletionTimestamp` is set (if not already) and the
    /// object is returned still present. Once finalizers are cleared, a delete
    /// actually removes it and emits a Deleted event.
    ///
    /// Returns `(object, removed)` where `removed` is true iff the object was
    /// actually deleted from the store.
    ///
    /// # Errors
    /// `NotFound`.
    pub fn delete(
        &mut self,
        gvr: &GroupVersionResource,
        namespace: &str,
        name: &str,
    ) -> Result<(Value, bool)> {
        let existing = self.get(gvr, namespace, name)?;
        let meta_now = meta::read_meta(&existing);
        let key = store_key(namespace, name);

        if meta_now.has_finalizers() {
            // Foreground/graceful deletion: mark deletionTimestamp, keep object.
            let mut object = existing;
            let mut m = meta_now;
            if m.deletion_timestamp.is_none() {
                let rv = self.next_rv();
                m.deletion_timestamp = Some(format!("delete-requested@rv-{rv}"));
                m.resource_version = rv.to_string();
                meta::write_meta(&mut object, &m);
                self.stores
                    .entry(gvr.clone())
                    .or_default()
                    .insert(key, object.clone());
                self.record(gvr, WatchEventKind::Modified, object.clone(), rv);
            }
            return Ok((object, false));
        }

        // No finalizers: actually remove.
        let rv = self.next_rv();
        if let Some(s) = self.stores.get_mut(gvr) {
            s.remove(&key);
        }
        self.record(gvr, WatchEventKind::Deleted, existing.clone(), rv);
        Ok((existing, true))
    }

    /// LIST with selectors + pagination. Items are deterministically ordered by
    /// `(namespace, name)`.
    ///
    /// # Errors
    /// `NotFound` for unknown resource, `BadRequest` for a bad continue token.
    pub fn list(&self, gvr: &GroupVersionResource, opts: &ListOptions) -> Result<ListResult> {
        Self::check_known(gvr)?;
        let mut matched: Vec<Value> = self
            .stores
            .get(gvr)
            .map(|s| {
                s.values()
                    .filter(|o| {
                        let m = meta::read_meta(o);
                        opts.namespace
                            .as_ref()
                            .map(|ns| &m.namespace == ns)
                            .unwrap_or(true)
                            && opts.label_selector.matches(&m.labels)
                            && opts.field_selector.matches(o)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        matched.sort_by(|a, b| {
            let ma = meta::read_meta(a);
            let mb = meta::read_meta(b);
            (ma.namespace, ma.name).cmp(&(mb.namespace, mb.name))
        });

        // Pagination: the continue token is the index to resume from.
        let start = match &opts.continue_token {
            Some(t) => t
                .parse::<usize>()
                .map_err(|_| Status::bad_request("invalid continue token"))?,
            None => 0,
        };
        if start > matched.len() {
            return Err(Status::bad_request("continue token out of range"));
        }
        let slice = &matched[start..];
        let (items, continue_token) = if opts.limit > 0 && slice.len() > opts.limit {
            let next = start + opts.limit;
            (slice[..opts.limit].to_vec(), Some(next.to_string()))
        } else {
            (slice.to_vec(), None)
        };

        Ok(ListResult {
            items,
            continue_token,
            resource_version: self.rv,
        })
    }

    /// WATCH replay: every event for `gvr` strictly newer than
    /// `after_resource_version`, in order.
    ///
    /// # Errors
    /// `NotFound` for unknown resource.
    pub fn watch_since(
        &self,
        gvr: &GroupVersionResource,
        after_resource_version: u64,
    ) -> Result<Vec<WatchEvent>> {
        Self::check_known(gvr)?;
        Ok(self
            .history
            .get(gvr)
            .map(|h| {
                h.iter()
                    .filter(|e| e.resource_version > after_resource_version)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Convenience: read metadata of a stored object (None if absent).
    #[must_use]
    pub fn meta_of(&self, gvr: &GroupVersionResource, namespace: &str, name: &str) -> Option<ObjectMeta> {
        self.stores
            .get(gvr)
            .and_then(|s| s.get(&store_key(namespace, name)))
            .map(meta::read_meta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::obj;

    fn pods() -> GroupVersionResource {
        GroupVersionResource::new("", "v1", "pods")
    }

    fn pod(ns: &str, name: &str) -> Value {
        obj([("metadata", obj([
            ("name", Value::from(name)),
            ("namespace", Value::from(ns)),
        ]))])
    }

    #[test]
    fn create_assigns_rv_uid_generation() {
        let mut r = Registry::new();
        let created = r.create(&pods(), pod("default", "nginx")).expect("create");
        let m = meta::read_meta(&created);
        assert_eq!(m.resource_version, "1");
        assert_eq!(m.generation, 1);
        assert!(m.uid.starts_with("uid-"));
    }

    #[test]
    fn create_duplicate_is_already_exists() {
        let mut r = Registry::new();
        r.create(&pods(), pod("default", "nginx")).expect("first");
        let err = r.create(&pods(), pod("default", "nginx")).expect_err("dup");
        assert_eq!(err.reason, StatusReason::AlreadyExists);
        assert_eq!(err.code, 409);
    }

    #[test]
    fn get_missing_is_not_found() {
        let r = Registry::new();
        let err = r.get(&pods(), "default", "ghost").expect_err("missing");
        assert_eq!(err.reason, StatusReason::NotFound);
        assert_eq!(err.code, 404);
    }

    #[test]
    fn unknown_resource_is_not_found() {
        let r = Registry::new();
        let gvr = GroupVersionResource::new("x.io", "v1", "widgets");
        assert_eq!(r.get(&gvr, "default", "a").unwrap_err().reason, StatusReason::NotFound);
    }

    #[test]
    fn update_with_stale_rv_conflicts() {
        let mut r = Registry::new();
        let created = r.create(&pods(), pod("default", "nginx")).expect("create");
        // Stale: pretend client still holds rv "0".
        let mut stale = created.clone();
        meta::write_meta(&mut stale, &ObjectMeta {
            name: "nginx".into(),
            namespace: "default".into(),
            resource_version: "999".into(),
            ..ObjectMeta::default()
        });
        let err = r.update(&pods(), stale).expect_err("conflict");
        assert_eq!(err.reason, StatusReason::Conflict);
        assert_eq!(err.code, 409);
    }

    #[test]
    fn update_with_current_rv_succeeds_and_bumps_rv() {
        let mut r = Registry::new();
        let created = r.create(&pods(), pod("default", "nginx")).expect("create");
        let updated = r.update(&pods(), created).expect("update");
        let m = meta::read_meta(&updated);
        assert_eq!(m.resource_version, "2");
    }

    #[test]
    fn generation_bumps_only_on_spec_change() {
        let mut r = Registry::new();
        let mut o = pod("default", "d");
        o.insert("spec", obj([("replicas", Value::from(1_i64))]));
        let created = r.create(&pods(), o).expect("create");
        assert_eq!(meta::read_meta(&created).generation, 1);

        // status-only change: no generation bump.
        let mut same_spec = created.clone();
        same_spec.insert("status", obj([("ready", Value::from(true))]));
        let u1 = r.update(&pods(), same_spec).expect("update1");
        assert_eq!(meta::read_meta(&u1).generation, 1);

        // spec change: generation bumps.
        let mut new_spec = u1.clone();
        new_spec.insert("spec", obj([("replicas", Value::from(3_i64))]));
        let u2 = r.update(&pods(), new_spec).expect("update2");
        assert_eq!(meta::read_meta(&u2).generation, 2);
    }

    #[test]
    fn delete_with_finalizers_sets_deletion_timestamp_and_keeps_object() {
        let mut r = Registry::new();
        let mut o = pod("default", "p");
        o.insert("metadata", obj([
            ("name", Value::from("p")),
            ("namespace", Value::from("default")),
            ("finalizers", Value::Array(vec![Value::from("foregroundDeletion")])),
        ]));
        r.create(&pods(), o).expect("create");
        let (obj_after, removed) = r.delete(&pods(), "default", "p").expect("delete");
        assert!(!removed, "object must NOT be removed while finalizers present");
        assert!(meta::read_meta(&obj_after).is_being_deleted());
        // Still gettable.
        assert!(r.get(&pods(), "default", "p").is_ok());
    }

    #[test]
    fn delete_after_finalizers_cleared_removes_object() {
        let mut r = Registry::new();
        let mut o = pod("default", "p");
        o.insert("metadata", obj([
            ("name", Value::from("p")),
            ("namespace", Value::from("default")),
            ("finalizers", Value::Array(vec![Value::from("foregroundDeletion")])),
        ]));
        let created = r.create(&pods(), o).expect("create");
        r.delete(&pods(), "default", "p").expect("mark");

        // Controller clears finalizers via update.
        let mut cleared = r.get(&pods(), "default", "p").expect("get");
        let mut m = meta::read_meta(&cleared);
        m.finalizers.clear();
        m.resource_version = meta::read_meta(&r.get(&pods(), "default", "p").unwrap()).resource_version;
        let _ = created;
        meta::write_meta(&mut cleared, &m);
        r.update(&pods(), cleared).expect("clear finalizers");

        let (_, removed) = r.delete(&pods(), "default", "p").expect("final delete");
        assert!(removed);
        assert!(r.get(&pods(), "default", "p").is_err());
    }

    #[test]
    fn delete_without_finalizers_removes_immediately() {
        let mut r = Registry::new();
        r.create(&pods(), pod("default", "p")).expect("create");
        let (_, removed) = r.delete(&pods(), "default", "p").expect("delete");
        assert!(removed);
        assert!(r.get(&pods(), "default", "p").is_err());
    }

    #[test]
    fn patch_merge_updates_field() {
        let mut r = Registry::new();
        let mut o = pod("default", "p");
        o.insert("spec", obj([("replicas", Value::from(1_i64))]));
        r.create(&pods(), o).expect("create");
        let patch = obj([("spec", obj([("replicas", Value::from(5_i64))]))]);
        let out = r.patch_merge(&pods(), "default", "p", &patch).expect("patch");
        assert_eq!(out.pointer("spec.replicas"), Some(&Value::from(5_i64)));
        assert_eq!(meta::read_meta(&out).generation, 2);
    }

    #[test]
    fn list_filters_by_label_selector() {
        let mut r = Registry::new();
        let mut a = pod("default", "a");
        a.insert("metadata", obj([
            ("name", Value::from("a")),
            ("namespace", Value::from("default")),
            ("labels", obj([("app", Value::from("web"))])),
        ]));
        r.create(&pods(), a).expect("a");
        r.create(&pods(), pod("default", "b")).expect("b");

        let mut opts = ListOptions::default();
        opts.label_selector = LabelSelector::parse("app=web").expect("sel");
        let res = r.list(&pods(), &opts).expect("list");
        assert_eq!(res.items.len(), 1);
        assert_eq!(meta::read_meta(&res.items[0]).name, "a");
    }

    #[test]
    fn list_filters_by_field_selector_namespace() {
        let mut r = Registry::new();
        r.create(&pods(), pod("ns1", "a")).expect("a");
        r.create(&pods(), pod("ns2", "b")).expect("b");
        let mut opts = ListOptions::default();
        opts.field_selector = FieldSelector::parse("metadata.namespace=ns1").expect("fs");
        let res = r.list(&pods(), &opts).expect("list");
        assert_eq!(res.items.len(), 1);
    }

    #[test]
    fn list_pagination_with_limit_and_continue() {
        let mut r = Registry::new();
        for n in ["a", "b", "c", "d", "e"] {
            r.create(&pods(), pod("default", n)).expect("create");
        }
        let mut opts = ListOptions { limit: 2, ..ListOptions::default() };
        let page1 = r.list(&pods(), &opts).expect("page1");
        assert_eq!(page1.items.len(), 2);
        assert_eq!(meta::read_meta(&page1.items[0]).name, "a");
        let token = page1.continue_token.clone().expect("more pages");

        opts.continue_token = Some(token);
        let page2 = r.list(&pods(), &opts).expect("page2");
        assert_eq!(page2.items.len(), 2);
        assert_eq!(meta::read_meta(&page2.items[0]).name, "c");

        opts.continue_token = page2.continue_token.clone();
        let page3 = r.list(&pods(), &opts).expect("page3");
        assert_eq!(page3.items.len(), 1);
        assert!(page3.continue_token.is_none());
    }

    #[test]
    fn list_namespace_scoping() {
        let mut r = Registry::new();
        r.create(&pods(), pod("ns1", "a")).expect("a");
        r.create(&pods(), pod("ns2", "b")).expect("b");
        let opts = ListOptions { namespace: Some("ns1".into()), ..ListOptions::default() };
        let res = r.list(&pods(), &opts).expect("list");
        assert_eq!(res.items.len(), 1);
        assert_eq!(meta::read_meta(&res.items[0]).namespace, "ns1");
    }

    #[test]
    fn watch_replays_events_after_rv() {
        let mut r = Registry::new();
        r.create(&pods(), pod("default", "a")).expect("a"); // rv 1
        r.create(&pods(), pod("default", "b")).expect("b"); // rv 2
        let events = r.watch_since(&pods(), 1).expect("watch");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, WatchEventKind::Added);
        assert_eq!(events[0].resource_version, 2);
    }

    #[test]
    fn watch_includes_modify_and_delete() {
        let mut r = Registry::new();
        let created = r.create(&pods(), pod("default", "a")).expect("a"); // rv1
        r.update(&pods(), created).expect("u"); // rv2
        r.delete(&pods(), "default", "a").expect("d"); // rv3
        let events = r.watch_since(&pods(), 0).expect("watch");
        let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![WatchEventKind::Added, WatchEventKind::Modified, WatchEventKind::Deleted]
        );
    }

    #[test]
    fn create_rejects_missing_name() {
        let mut r = Registry::new();
        let err = r.create(&pods(), obj([("metadata", Value::object())])).expect_err("no name");
        assert_eq!(err.reason, StatusReason::Invalid);
    }

    // --- /status subresource semantics (Kubernetes API conventions) ---------

    #[test]
    fn update_status_persists_only_status_and_ignores_spec() {
        let mut r = Registry::new();
        let mut o = pod("default", "web");
        o.insert("spec", obj([("replicas", Value::from(1_i64))]));
        let created = r.create(&pods(), o).expect("create"); // rv1, gen1

        // A status write that ALSO tries to change spec: spec must be ignored.
        let mut submit = created.clone();
        submit.insert("spec", obj([("replicas", Value::from(99_i64))]));
        submit.insert("status", obj([("readyReplicas", Value::from(1_i64))]));

        let out = r.update_status(&pods(), submit).expect("update_status");
        // Spec is preserved from the stored object, not the client's submission.
        assert_eq!(out.pointer("spec.replicas"), Some(&Value::from(1_i64)));
        // Status is taken from the submission.
        assert_eq!(out.pointer("status.readyReplicas"), Some(&Value::from(1_i64)));
    }

    #[test]
    fn update_status_bumps_rv_but_never_generation() {
        let mut r = Registry::new();
        let mut o = pod("default", "web");
        o.insert("spec", obj([("replicas", Value::from(2_i64))]));
        let created = r.create(&pods(), o).expect("create"); // rv1 gen1
        assert_eq!(meta::read_meta(&created).generation, 1);

        let mut submit = created.clone();
        submit.insert("status", obj([("ready", Value::from(true))]));
        let out = r.update_status(&pods(), submit).expect("update_status");
        let m = meta::read_meta(&out);
        assert_eq!(m.resource_version, "2"); // rv bumped
        assert_eq!(m.generation, 1); // generation NOT bumped by a status write
    }

    #[test]
    fn update_status_with_stale_rv_conflicts() {
        let mut r = Registry::new();
        let created = r.create(&pods(), pod("default", "web")).expect("create");
        let mut stale = created.clone();
        meta::write_meta(&mut stale, &ObjectMeta {
            name: "web".into(),
            namespace: "default".into(),
            resource_version: "999".into(),
            ..ObjectMeta::default()
        });
        stale.insert("status", obj([("ready", Value::from(true))]));
        let err = r.update_status(&pods(), stale).expect_err("conflict");
        assert_eq!(err.reason, StatusReason::Conflict);
        assert_eq!(err.code, 409);
    }

    #[test]
    fn update_status_on_missing_object_is_not_found() {
        let mut r = Registry::new();
        let mut o = pod("default", "ghost");
        o.insert("status", obj([("ready", Value::from(true))]));
        let err = r.update_status(&pods(), o).expect_err("missing");
        assert_eq!(err.reason, StatusReason::NotFound);
        assert_eq!(err.code, 404);
    }

    #[test]
    fn update_status_emits_modified_watch_event() {
        let mut r = Registry::new();
        let created = r.create(&pods(), pod("default", "web")).expect("create"); // rv1 Added
        let mut submit = created.clone();
        submit.insert("status", obj([("ready", Value::from(true))]));
        r.update_status(&pods(), submit).expect("update_status"); // rv2 Modified
        let events = r.watch_since(&pods(), 1).expect("watch");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, WatchEventKind::Modified);
        assert_eq!(events[0].resource_version, 2);
    }
}
