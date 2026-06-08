// SPDX-License-Identifier: Apache-2.0
//! The informer cache model.
//!
//! Provides a [`Store`] (a thread-confined indexer keyed by `namespace/name`
//! with by-namespace and by-label indices) and a [`DeltaFifo`] delta queue
//! producing [`Delta`] events (Added / Updated / Deleted / Sync).
//!
//! Behavioural reimplementation of the documented contract of client-go's
//! `tools/cache` (`Store`, `Indexer`, `DeltaFIFO`). The actual `Reflector` /
//! `ListWatch` that fills the store from the apiserver is **deferred** — this
//! module is pure over events *handed in* by the caller, which is exactly the
//! part that benefits from exhaustive testing. `std` only.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::types::Object;

/// The kind of change observed for an object (client-go `DeltaType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaType {
    /// Object newly observed.
    Added,
    /// Object observed again with (possibly) changed content.
    Updated,
    /// Object observed as gone.
    Deleted,
    /// Object replayed during a relist/resync (no content change implied).
    Sync,
}

/// A single observed change: its [`DeltaType`] and the object it concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta<T> {
    /// What kind of change this is.
    pub delta_type: DeltaType,
    /// The object (last-known content for `Deleted`).
    pub object: T,
}

/// A namespace/name-keyed object cache with secondary indices.
///
/// Indices maintained:
/// * **by namespace** — `namespace -> {keys}`
/// * **by label** — `"key=value" -> {keys}` for every label on every object
///
/// Indices are kept consistent on every mutation. Lookups return owned clones
/// so callers cannot mutate the cache through a reference.
#[derive(Debug, Clone)]
pub struct Store<T: Object> {
    items: HashMap<String, T>,
    by_namespace: HashMap<String, BTreeSet<String>>,
    by_label: HashMap<String, BTreeSet<String>>,
}

impl<T: Object> Default for Store<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Object> Store<T> {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
            by_namespace: HashMap::new(),
            by_label: HashMap::new(),
        }
    }

    fn label_index_keys(obj: &T) -> Vec<String> {
        obj.meta()
            .labels
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect()
    }

    fn deindex(&mut self, key: &str, obj: &T) {
        let ns = &obj.meta().namespace;
        if let Some(set) = self.by_namespace.get_mut(ns) {
            set.remove(key);
            if set.is_empty() {
                self.by_namespace.remove(ns);
            }
        }
        for lk in Self::label_index_keys(obj) {
            if let Some(set) = self.by_label.get_mut(&lk) {
                set.remove(key);
                if set.is_empty() {
                    self.by_label.remove(&lk);
                }
            }
        }
    }

    fn index(&mut self, key: &str, obj: &T) {
        self.by_namespace
            .entry(obj.meta().namespace.clone())
            .or_default()
            .insert(key.to_owned());
        for lk in Self::label_index_keys(obj) {
            self.by_label.entry(lk).or_default().insert(key.to_owned());
        }
    }

    /// Insert or replace an object, keeping every index consistent.
    pub fn upsert(&mut self, obj: T) {
        let key = obj.key();
        if let Some(old) = self.items.remove(&key) {
            self.deindex(&key, &old);
        }
        self.index(&key, &obj);
        self.items.insert(key, obj);
    }

    /// Remove an object by key. Returns the removed object, if any.
    pub fn remove(&mut self, key: &str) -> Option<T> {
        let obj = self.items.remove(key)?;
        self.deindex(key, &obj);
        Some(obj)
    }

    /// Fetch an object by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<T> {
        self.items.get(key).cloned()
    }

    /// Number of objects held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` if the store holds no objects.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Every key currently in the store, sorted.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        let mut ks: Vec<String> = self.items.keys().cloned().collect();
        ks.sort();
        ks
    }

    /// Every object, in key-sorted order.
    #[must_use]
    pub fn list(&self) -> Vec<T> {
        self.keys()
            .into_iter()
            .filter_map(|k| self.items.get(&k).cloned())
            .collect()
    }

    /// Objects in a namespace, in key-sorted order (uses the namespace index).
    #[must_use]
    pub fn list_by_namespace(&self, namespace: &str) -> Vec<T> {
        self.by_namespace
            .get(namespace)
            .into_iter()
            .flatten()
            .filter_map(|k| self.items.get(k).cloned())
            .collect()
    }

    /// Objects carrying `label_key == label_value` (uses the label index),
    /// in key-sorted order.
    #[must_use]
    pub fn list_by_label(&self, label_key: &str, label_value: &str) -> Vec<T> {
        let lk = format!("{label_key}={label_value}");
        self.by_label
            .get(&lk)
            .into_iter()
            .flatten()
            .filter_map(|k| self.items.get(k).cloned())
            .collect()
    }

    /// Objects matching **all** of `selector` (label-selector AND semantics),
    /// in key-sorted order. An empty selector matches everything.
    #[must_use]
    pub fn list_matching(&self, selector: &BTreeMap<String, String>) -> Vec<T> {
        self.list()
            .into_iter()
            .filter(|o| {
                selector
                    .iter()
                    .all(|(k, v)| o.meta().labels.get(k) == Some(v))
            })
            .collect()
    }
}

/// A delta queue that compresses repeated changes per key and tracks the
/// known-object set, mirroring client-go's `DeltaFIFO` contract.
///
/// The caller feeds observed objects (`add`/`update`/`delete`) and periodically
/// `replace`s the whole set during a relist; [`DeltaFifo::pop`] yields one
/// key's accumulated deltas in arrival order. Deltas for one key are
/// **coalesced** so a flood of updates to the same object does not unbounded-
/// grow the queue (the documented "deltas are compressed" behaviour: we keep
/// the ordered list but drop an `Updated` that immediately follows an
/// `Added`/`Updated` of the same key by collapsing to the latest object).
#[derive(Debug, Clone)]
pub struct DeltaFifo<T: Object> {
    /// Per-key ordered deltas awaiting pop.
    queues: HashMap<String, Vec<Delta<T>>>,
    /// FIFO order of keys with pending deltas.
    order: Vec<String>,
    /// Keys the FIFO believes currently exist (for relist-driven deletes).
    known: BTreeSet<String>,
    /// Last-known object per key, retained so a relist can synthesise a
    /// `Deleted` delta (carrying the last content) for a key that vanished
    /// even after its earlier deltas were popped.
    last_known: HashMap<String, T>,
}

impl<T: Object> Default for DeltaFifo<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Object> DeltaFifo<T> {
    /// An empty delta FIFO.
    #[must_use]
    pub fn new() -> Self {
        Self {
            queues: HashMap::new(),
            order: Vec::new(),
            known: BTreeSet::new(),
            last_known: HashMap::new(),
        }
    }

    fn enqueue(&mut self, key: &str, delta: Delta<T>) {
        if !self.queues.contains_key(key) {
            self.queues.insert(key.to_owned(), Vec::new());
            self.order.push(key.to_owned());
        }
        // The key is now guaranteed present; `get_or_insert` keeps this total
        // (no panic path) even though the branch above ensured presence.
        let q = self.queues.entry(key.to_owned()).or_default();
        // Coalesce: a trailing Added/Updated followed by another Added/Updated
        // for the same key collapses to the newer object. A Deleted is always
        // recorded (it is terminal information).
        if let Some(last) = q.last_mut() {
            let mergeable = matches!(
                (last.delta_type, delta.delta_type),
                (DeltaType::Added | DeltaType::Updated | DeltaType::Sync,
                 DeltaType::Added | DeltaType::Updated | DeltaType::Sync)
            );
            if mergeable {
                last.object = delta.object;
                // keep the more-specific Updated type once an update arrives
                if delta.delta_type == DeltaType::Updated {
                    last.delta_type = DeltaType::Updated;
                }
                return;
            }
        }
        q.push(delta);
    }

    /// Record an observed add.
    pub fn add(&mut self, obj: T) {
        let key = obj.key();
        self.known.insert(key.clone());
        self.last_known.insert(key.clone(), obj.clone());
        self.enqueue(&key, Delta { delta_type: DeltaType::Added, object: obj });
    }

    /// Record an observed update.
    pub fn update(&mut self, obj: T) {
        let key = obj.key();
        self.known.insert(key.clone());
        self.last_known.insert(key.clone(), obj.clone());
        self.enqueue(&key, Delta { delta_type: DeltaType::Updated, object: obj });
    }

    /// Record an observed delete (carries the last-known object).
    pub fn delete(&mut self, obj: T) {
        let key = obj.key();
        self.known.remove(&key);
        self.last_known.remove(&key);
        self.enqueue(&key, Delta { delta_type: DeltaType::Deleted, object: obj });
    }

    /// Replace the entire known set during a relist: every supplied object is
    /// enqueued as a `Sync`, and any previously-known key not present in the
    /// new set is enqueued as a `Deleted` (the relist-detects-deletion
    /// contract).
    pub fn replace(&mut self, objects: Vec<T>) {
        let new_keys: BTreeSet<String> = objects.iter().map(Object::key).collect();
        // Deletes for keys that vanished.
        let vanished: Vec<String> = self
            .known
            .iter()
            .filter(|k| !new_keys.contains(*k))
            .cloned()
            .collect();
        for key in vanished {
            if let Some(obj) = self.last_known.remove(&key) {
                self.enqueue(&key, Delta { delta_type: DeltaType::Deleted, object: obj });
            }
        }
        for obj in objects {
            let key = obj.key();
            self.last_known.insert(key.clone(), obj.clone());
            self.enqueue(&key, Delta { delta_type: DeltaType::Sync, object: obj });
        }
        self.known = new_keys;
    }

    /// Pop the oldest key's accumulated deltas (FIFO over keys). `None` if
    /// empty. The returned `Vec` is non-empty and in arrival order.
    pub fn pop(&mut self) -> Option<(String, Vec<Delta<T>>)> {
        while let Some(key) = {
            if self.order.is_empty() {
                None
            } else {
                Some(self.order.remove(0))
            }
        } {
            if let Some(deltas) = self.queues.remove(&key) {
                if !deltas.is_empty() {
                    return Some((key, deltas));
                }
            }
        }
        None
    }

    /// Number of keys with pending deltas.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// `true` if there is nothing to pop.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Apply one [`Delta`] to a [`Store`], the standard "informer keeps the cache
/// in sync with the delta stream" reducer.
pub fn apply_delta<T: Object>(store: &mut Store<T>, delta: &Delta<T>) {
    match delta.delta_type {
        DeltaType::Added | DeltaType::Updated | DeltaType::Sync => {
            store.upsert(delta.object.clone());
        }
        DeltaType::Deleted => {
            store.remove(&delta.object.key());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ObjectMeta;

    fn obj(name: &str, ns: &str) -> ObjectMeta {
        ObjectMeta::new(name, ns, &format!("{ns}-{name}"))
    }

    #[test]
    fn store_upsert_get_remove() {
        let mut s: Store<ObjectMeta> = Store::new();
        s.upsert(obj("a", "ns1"));
        assert_eq!(s.len(), 1);
        assert_eq!(s.get("ns1/a").map(|o| o.uid), Some("ns1-a".to_owned()));
        assert!(s.remove("ns1/a").is_some());
        assert!(s.is_empty());
    }

    #[test]
    fn store_namespace_index() {
        let mut s: Store<ObjectMeta> = Store::new();
        s.upsert(obj("a", "ns1"));
        s.upsert(obj("b", "ns1"));
        s.upsert(obj("c", "ns2"));
        let ns1: Vec<_> = s.list_by_namespace("ns1").into_iter().map(|o| o.name).collect();
        assert_eq!(ns1, vec!["a", "b"]);
        assert_eq!(s.list_by_namespace("ns2").len(), 1);
        assert_eq!(s.list_by_namespace("none").len(), 0);
    }

    #[test]
    fn store_label_index() {
        let mut s: Store<ObjectMeta> = Store::new();
        s.upsert(obj("a", "ns").with_label("app", "web"));
        s.upsert(obj("b", "ns").with_label("app", "web"));
        s.upsert(obj("c", "ns").with_label("app", "db"));
        let web: Vec<_> = s.list_by_label("app", "web").into_iter().map(|o| o.name).collect();
        assert_eq!(web, vec!["a", "b"]);
        assert_eq!(s.list_by_label("app", "db").len(), 1);
    }

    #[test]
    fn store_label_index_updates_on_relabel() {
        let mut s: Store<ObjectMeta> = Store::new();
        s.upsert(obj("a", "ns").with_label("app", "web"));
        assert_eq!(s.list_by_label("app", "web").len(), 1);
        // relabel a -> db; the old index entry must be cleaned up
        s.upsert(obj("a", "ns").with_label("app", "db"));
        assert_eq!(s.list_by_label("app", "web").len(), 0);
        assert_eq!(s.list_by_label("app", "db").len(), 1);
    }

    #[test]
    fn store_list_matching_is_and_semantics() {
        let mut s: Store<ObjectMeta> = Store::new();
        s.upsert(obj("a", "ns").with_label("app", "web").with_label("tier", "fe"));
        s.upsert(obj("b", "ns").with_label("app", "web").with_label("tier", "be"));
        let mut sel = BTreeMap::new();
        sel.insert("app".to_owned(), "web".to_owned());
        sel.insert("tier".to_owned(), "fe".to_owned());
        let m: Vec<_> = s.list_matching(&sel).into_iter().map(|o| o.name).collect();
        assert_eq!(m, vec!["a"]);
        assert_eq!(s.list_matching(&BTreeMap::new()).len(), 2, "empty selector matches all");
    }

    #[test]
    fn fifo_emits_added_then_deleted() {
        let mut f: DeltaFifo<ObjectMeta> = DeltaFifo::new();
        f.add(obj("a", "ns"));
        f.delete(obj("a", "ns"));
        let (key, deltas) = f.pop().expect("pending");
        assert_eq!(key, "ns/a");
        let types: Vec<_> = deltas.iter().map(|d| d.delta_type).collect();
        assert_eq!(types, vec![DeltaType::Added, DeltaType::Deleted]);
    }

    #[test]
    fn fifo_coalesces_repeated_updates() {
        let mut f: DeltaFifo<ObjectMeta> = DeltaFifo::new();
        f.add(obj("a", "ns"));
        f.update(obj("a", "ns").with_label("v", "2"));
        f.update(obj("a", "ns").with_label("v", "3"));
        let (_key, deltas) = f.pop().expect("pending");
        assert_eq!(deltas.len(), 1, "Added+Updated+Updated coalesce to one");
        assert_eq!(deltas[0].delta_type, DeltaType::Updated);
        assert_eq!(deltas[0].object.labels.get("v").map(String::as_str), Some("3"));
    }

    #[test]
    fn fifo_is_fifo_over_keys() {
        let mut f: DeltaFifo<ObjectMeta> = DeltaFifo::new();
        f.add(obj("first", "ns"));
        f.add(obj("second", "ns"));
        assert_eq!(f.pop().expect("k").0, "ns/first");
        assert_eq!(f.pop().expect("k").0, "ns/second");
        assert!(f.pop().is_none());
    }

    #[test]
    fn fifo_replace_syncs_present_and_deletes_vanished() {
        let mut f: DeltaFifo<ObjectMeta> = DeltaFifo::new();
        f.add(obj("a", "ns"));
        f.add(obj("b", "ns"));
        // drain the initial adds
        while f.pop().is_some() {}
        // relist sees only "a" and a new "c": b vanished.
        f.replace(vec![obj("a", "ns"), obj("c", "ns")]);
        let mut got: Vec<(String, DeltaType)> = Vec::new();
        while let Some((k, ds)) = f.pop() {
            for d in ds {
                got.push((k.clone(), d.delta_type));
            }
        }
        assert!(got.contains(&("ns/b".to_owned(), DeltaType::Deleted)), "vanished b deleted");
        assert!(got.contains(&("ns/a".to_owned(), DeltaType::Sync)), "present a synced");
        assert!(got.contains(&("ns/c".to_owned(), DeltaType::Sync)), "new c synced");
    }

    #[test]
    fn apply_delta_keeps_store_in_sync() {
        let mut s: Store<ObjectMeta> = Store::new();
        apply_delta(&mut s, &Delta { delta_type: DeltaType::Added, object: obj("a", "ns") });
        assert_eq!(s.len(), 1);
        apply_delta(&mut s, &Delta { delta_type: DeltaType::Deleted, object: obj("a", "ns") });
        assert!(s.is_empty());
    }
}
