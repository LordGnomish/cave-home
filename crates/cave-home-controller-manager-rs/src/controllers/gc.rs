// SPDX-License-Identifier: Apache-2.0
//! Garbage collector — ownerReference-based cascading deletion.
//!
//! Behavioural reimplementation of the documented kube-controller-manager
//! garbage-collector contract (`pkg/controller/garbagecollector`): an object
//! whose owners are all gone becomes an orphan and is collected; deletion
//! cascades down the owner graph. `std` only, pure over a passed-in object set.
//!
//! Modelled rules (each tested):
//! * an object with **no** owner references is a root and is never GC'd by
//!   ownership (it is collected only by explicit deletion);
//! * an object is collected when **every** owner UID it references is absent
//!   from the live set (the "all owners gone" rule);
//! * deletion **cascades**: collecting an object can orphan its own dependents,
//!   which are then collected in the same sweep (transitive closure);
//! * **foreground deletion** — when a root is marked terminating, dependents
//!   whose owner ref has `block_owner_deletion` keep the root alive until those
//!   dependents are gone; the dependents are scheduled for deletion first.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::types::{Object, Uid};

/// A node in the ownership graph.
#[derive(Debug, Clone)]
struct Node {
    uid: Uid,
    /// UIDs this object points at via ownerReferences.
    owners: Vec<Uid>,
    /// Whether any owner ref carries `blockOwnerDeletion`.
    has_blocking_owner: bool,
    /// Whether the object itself is marked terminating.
    terminating: bool,
}

/// The ownership graph built from a set of objects, plus the GC decision.
#[derive(Debug, Default)]
pub struct OwnerGraph {
    nodes: HashMap<Uid, Node>,
    /// uid -> set of dependent uids (reverse of `owners`).
    dependents: HashMap<Uid, BTreeSet<Uid>>,
}

impl OwnerGraph {
    /// Build the owner graph from an object set.
    ///
    /// Each object contributes a node keyed by its UID. Objects with empty UIDs
    /// are skipped (they are not yet persisted and cannot be referenced).
    #[must_use]
    pub fn build<T: Object>(objects: &[T]) -> Self {
        let mut nodes: HashMap<Uid, Node> = HashMap::new();
        let mut dependents: HashMap<Uid, BTreeSet<Uid>> = HashMap::new();
        for obj in objects {
            let meta = obj.meta();
            if meta.uid.is_empty() {
                continue;
            }
            let owners: Vec<Uid> = meta.owner_references.iter().map(|o| o.uid.clone()).collect();
            let has_blocking_owner = meta
                .owner_references
                .iter()
                .any(|o| o.block_owner_deletion);
            for owner in &owners {
                dependents
                    .entry(owner.clone())
                    .or_default()
                    .insert(meta.uid.clone());
            }
            nodes.insert(
                meta.uid.clone(),
                Node {
                    uid: meta.uid.clone(),
                    owners,
                    has_blocking_owner,
                    terminating: meta.is_terminating(),
                },
            );
        }
        Self { nodes, dependents }
    }

    /// UIDs of every direct dependent of `uid`, sorted.
    #[must_use]
    pub fn dependents_of(&self, uid: &str) -> Vec<Uid> {
        self.dependents
            .get(uid)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Compute the set of UIDs that should be deleted.
    ///
    /// A node is collected if it has at least one owner reference and **all** of
    /// its owners are absent from the live graph. Collection is transitive: a
    /// node orphaned only because a to-be-collected node disappears is also
    /// collected. Roots that are explicitly `terminating` are included, and
    /// their dependents are cascaded.
    #[must_use]
    pub fn delete_set(&self) -> BTreeSet<Uid> {
        let mut deleted: HashSet<Uid> = HashSet::new();

        // Seed: explicitly-terminating nodes are going away; cascade from them.
        let mut frontier: Vec<Uid> = self
            .nodes
            .values()
            .filter(|n| n.terminating)
            .map(|n| n.uid.clone())
            .collect();
        while let Some(uid) = frontier.pop() {
            if deleted.insert(uid.clone()) {
                for dep in self.dependents_of(&uid) {
                    frontier.push(dep);
                }
            }
        }

        // Fixpoint over "all owners gone" orphan rule.
        loop {
            let mut changed = false;
            let candidates: Vec<Uid> = self
                .nodes
                .values()
                .filter(|n| !deleted.contains(&n.uid))
                .filter(|n| !n.owners.is_empty())
                .filter(|n| {
                    // collected iff every owner is missing from live or already deleted
                    n.owners.iter().all(|o| {
                        !self.nodes.contains_key(o) || deleted.contains(o)
                    })
                })
                .map(|n| n.uid.clone())
                .collect();
            for uid in candidates {
                if deleted.insert(uid) {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        deleted.into_iter().collect()
    }

    /// Foreground-deletion blockers: of the dependents that block deletion of
    /// the given (terminating) root `uid`, which are still present.
    ///
    /// While this returns a non-empty set, the root must not be finalized: its
    /// `block_owner_deletion` dependents must be removed first. Returns sorted
    /// UIDs.
    #[must_use]
    pub fn foreground_blockers(&self, uid: &str) -> Vec<Uid> {
        self.dependents_of(uid)
            .into_iter()
            .filter(|dep| {
                self.nodes
                    .get(dep)
                    .is_some_and(|n| n.has_blocking_owner)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ObjectMeta, OwnerReference};

    fn root(uid: &str) -> ObjectMeta {
        ObjectMeta::new(uid, "ns", uid)
    }

    fn owned_by(uid: &str, owner_uid: &str) -> ObjectMeta {
        ObjectMeta::new(uid, "ns", uid).with_owner(OwnerReference::to("Owner", owner_uid, owner_uid))
    }

    #[test]
    fn rootless_object_is_never_collected_by_ownership() {
        let g = OwnerGraph::build(&[root("a")]);
        assert!(g.delete_set().is_empty());
    }

    #[test]
    fn object_with_present_owner_survives() {
        let objs = vec![root("owner"), owned_by("dep", "owner")];
        let g = OwnerGraph::build(&objs);
        assert!(g.delete_set().is_empty());
    }

    #[test]
    fn orphan_with_missing_owner_is_collected() {
        // "dep" references "owner", but owner is not in the set.
        let g = OwnerGraph::build(&[owned_by("dep", "owner")]);
        let del = g.delete_set();
        assert!(del.contains("dep"));
        assert_eq!(del.len(), 1);
    }

    #[test]
    fn object_with_one_present_owner_among_several_survives() {
        let dep = ObjectMeta::new("dep", "ns", "dep")
            .with_owner(OwnerReference::to("O", "gone", "gone"))
            .with_owner(OwnerReference::to("O", "owner", "owner"));
        let g = OwnerGraph::build(&[root("owner"), dep]);
        assert!(g.delete_set().is_empty(), "one live owner keeps it alive");
    }

    #[test]
    fn cascading_delete_collects_transitive_dependents() {
        // owner (terminating) -> child -> grandchild
        let mut owner = root("owner");
        owner.deletion_timestamp = Some(10);
        let child = owned_by("child", "owner");
        let grandchild = owned_by("grandchild", "child");
        let g = OwnerGraph::build(&[owner, child, grandchild]);
        let del = g.delete_set();
        assert!(del.contains("owner"));
        assert!(del.contains("child"));
        assert!(del.contains("grandchild"), "delete cascades transitively");
        assert_eq!(del.len(), 3);
    }

    #[test]
    fn cascade_does_not_touch_unrelated_trees() {
        let mut owner = root("owner");
        owner.deletion_timestamp = Some(10);
        let child = owned_by("child", "owner");
        let other = root("other");
        let other_child = owned_by("other-child", "other");
        let g = OwnerGraph::build(&[owner, child, other, other_child]);
        let del = g.delete_set();
        assert!(del.contains("owner") && del.contains("child"));
        assert!(!del.contains("other") && !del.contains("other-child"));
    }

    #[test]
    fn dependents_of_lists_direct_children() {
        let g = OwnerGraph::build(&[
            root("owner"),
            owned_by("c1", "owner"),
            owned_by("c2", "owner"),
        ]);
        assert_eq!(g.dependents_of("owner"), vec!["c1".to_owned(), "c2".to_owned()]);
    }

    #[test]
    fn foreground_blockers_reports_blocking_dependents() {
        let mut owner = root("owner");
        owner.deletion_timestamp = Some(5);
        let blocking = ObjectMeta::new("b", "ns", "b")
            .with_owner(OwnerReference::to("O", "owner", "owner").blocking());
        let nonblocking = owned_by("nb", "owner");
        let g = OwnerGraph::build(&[owner, blocking, nonblocking]);
        assert_eq!(g.foreground_blockers("owner"), vec!["b".to_owned()]);
    }

    #[test]
    fn empty_uid_objects_are_ignored() {
        let no_uid = ObjectMeta::new("x", "ns", "");
        let g = OwnerGraph::build(&[no_uid]);
        assert!(g.delete_set().is_empty());
    }
}
