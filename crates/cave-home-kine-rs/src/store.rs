// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The revisioned key/value model — kine's append-only row log.
//!
//! kine does not overwrite rows. Every mutation appends a new row to a single
//! table; the "current" value of a key is the latest non-deleted row for it.
//! This is exactly how kine emulates etcd MVCC on top of a plain SQL table.
//! Each row records:
//!
//! | field             | meaning                                              |
//! |-------------------|------------------------------------------------------|
//! | `key`             | the etcd key                                         |
//! | `create_revision` | revision at which the key was (re)created            |
//! | `mod_revision`    | revision at which this row was written               |
//! | `value`           | the stored bytes (empty for a tombstone)             |
//! | `lease`           | attached lease id, `0` = none                        |
//! | `deleted`         | tombstone flag — `true` rows mark a delete           |
//!
//! The SQL shape of this row is modelled in [`crate::sql`]. Here it is pure
//! in-memory logic: an append-only `Vec<Row>` plus a [`Clock`].
//!
//! Reference: `k3s-io/kine` `pkg/server` + `pkg/drivers/generic` (the
//! `create_revision` / `mod_revision` / `deleted` columns) and etcd MVCC
//! key-index semantics. Behavioural reimplementation; Apache-2.0.

use crate::error::{KineError, Result};
use crate::revision::{Clock, Revision};

/// One immutable row in the append-only log. A key's history is the ordered
/// sequence of rows that share its `key`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The key these bytes belong to.
    pub key: Vec<u8>,
    /// Revision at which the key was created in the generation this row belongs
    /// to. Re-creating a deleted key starts a new generation with a new
    /// `create_revision`, matching etcd.
    pub create_revision: Revision,
    /// Revision at which this specific row was written. Unique per row.
    pub mod_revision: Revision,
    /// The stored value. Empty for a tombstone (`deleted == true`).
    pub value: Vec<u8>,
    /// Attached lease id; `0` means no lease.
    pub lease: i64,
    /// Tombstone flag. A `true` row records a delete event at `mod_revision`.
    pub deleted: bool,
}

impl Row {
    /// The version of the key as of this row: how many writes have occurred in
    /// the current generation. etcd's `KeyValue.version` starts at `1` on
    /// create and increments on each update; a tombstone resets it.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        !self.deleted
    }
}

/// The append-only revisioned store: the heart of kine's etcd emulation.
///
/// All mutations go through [`Self::create`], [`Self::update`] and
/// [`Self::delete`], each of which appends exactly one row and bumps the global
/// revision once. Reads never mutate.
#[derive(Debug, Clone, Default)]
pub struct Store {
    clock: Clock,
    rows: Vec<Row>,
    /// Compacted floor: history at or below this revision (except the single
    /// surviving live row per key) has been dropped, and reads below it are
    /// rejected. `0` means "never compacted".
    compacted: Revision,
}

impl Store {
    /// A fresh, empty store at revision `0`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The store's current (header) revision.
    #[must_use]
    pub const fn current_revision(&self) -> Revision {
        self.clock.current()
    }

    /// The compacted-revision floor (`0` if never compacted).
    #[must_use]
    pub const fn compacted_revision(&self) -> Revision {
        self.compacted
    }

    /// Read-only access to the raw row log, oldest first. Intended for the
    /// range / watch / compaction layers, which are pure functions over it.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The clock, for the read layer's revision resolution.
    #[must_use]
    pub const fn clock(&self) -> &Clock {
        &self.clock
    }

    /// The latest row written for `key`, if the key has ever existed (live or
    /// tombstoned). `None` if the key was never written.
    fn latest_row(&self, key: &[u8]) -> Option<&Row> {
        self.rows.iter().rev().find(|r| r.key == key)
    }

    /// The current live value of `key`: the latest row, but only if it is not a
    /// tombstone. This is the "current-state view" for a single key.
    #[must_use]
    pub fn get_live(&self, key: &[u8]) -> Option<&Row> {
        self.latest_row(key).filter(|r| r.is_live())
    }

    /// Create a key that does not currently exist (or was deleted). Appends a
    /// fresh-generation row whose `create_revision == mod_revision == rev`.
    ///
    /// Mirrors kine's `Create`, which is a conditional insert: it succeeds only
    /// when the key has no live row. Returns the new revision.
    ///
    /// # Errors
    /// * [`KineError::EmptyKey`] if `key` is empty.
    /// * [`KineError::InvalidLeaseId`] is **not** raised here: lease `0` (no
    ///   lease) is valid on a plain create; use [`crate::lease`] to attach a
    ///   real lease. A negative lease is clamped by the type system (`i64`),
    ///   and `0` is the documented "no lease" sentinel.
    ///
    /// Returns `Ok(None)` (no write) if the key already has a live row — the
    /// caller decides whether that is an error, matching etcd's txn-compare
    /// idiom where `Create` is "put if `create_revision` == 0".
    pub fn create(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Option<Revision>> {
        if key.is_empty() {
            return Err(KineError::EmptyKey);
        }
        if self.get_live(key).is_some() {
            return Ok(None);
        }
        let rev = self.clock.next();
        self.rows.push(Row {
            key: key.to_vec(),
            create_revision: rev,
            mod_revision: rev,
            value: value.to_vec(),
            lease,
            deleted: false,
        });
        Ok(Some(rev))
    }

    /// Update a key that currently has a live row. Appends a new row that keeps
    /// the existing `create_revision` (same generation) and sets
    /// `mod_revision` to the new revision.
    ///
    /// Returns `Ok(None)` (no write) if the key has no live row, so the caller
    /// can distinguish "updated" from "absent" like an etcd compare-and-put.
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] if `key` is empty.
    pub fn update(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Option<Revision>> {
        if key.is_empty() {
            return Err(KineError::EmptyKey);
        }
        let Some(create_revision) = self.get_live(key).map(|r| r.create_revision) else {
            return Ok(None);
        };
        let rev = self.clock.next();
        self.rows.push(Row {
            key: key.to_vec(),
            create_revision,
            mod_revision: rev,
            value: value.to_vec(),
            lease,
            deleted: false,
        });
        Ok(Some(rev))
    }

    /// Put: create the key if absent, else update it. This is the unconditional
    /// `Put` etcd clients use most; it always writes a row. Returns the new
    /// revision.
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] if `key` is empty.
    pub fn put(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Revision> {
        if key.is_empty() {
            return Err(KineError::EmptyKey);
        }
        let create_revision = self.get_live(key).map(|r| r.create_revision);
        let rev = self.clock.next();
        self.rows.push(Row {
            key: key.to_vec(),
            create_revision: create_revision.unwrap_or(rev),
            mod_revision: rev,
            value: value.to_vec(),
            lease,
            deleted: false,
        });
        Ok(rev)
    }

    /// Delete a key by appending a tombstone row at a fresh revision. The
    /// tombstone keeps the dying generation's `create_revision` so a watcher
    /// can attribute the DELETE event to the right generation.
    ///
    /// Returns `Ok(None)` (no write) if the key has no live row — deleting an
    /// absent key is a no-op in etcd (it reports `deleted == 0`).
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] if `key` is empty.
    pub fn delete(&mut self, key: &[u8]) -> Result<Option<Revision>> {
        if key.is_empty() {
            return Err(KineError::EmptyKey);
        }
        let Some(create_revision) = self.get_live(key).map(|r| r.create_revision) else {
            return Ok(None);
        };
        let rev = self.clock.next();
        self.rows.push(Row {
            key: key.to_vec(),
            create_revision,
            mod_revision: rev,
            value: Vec::new(),
            lease: 0,
            deleted: true,
        });
        Ok(Some(rev))
    }

    /// Replace the row log and compacted floor — used by [`crate::compact`]
    /// after it computes the surviving rows. Internal to the crate.
    pub(crate) fn install_compacted(&mut self, rows: Vec<Row>, compacted: Revision) {
        self.rows = rows;
        self.compacted = compacted;
    }

    /// All distinct keys that currently have a live row, sorted lexically — the
    /// full current-state key set. Useful for tests and for the range layer.
    #[must_use]
    pub fn live_keys(&self) -> Vec<Vec<u8>> {
        let mut keys: Vec<Vec<u8>> = Vec::new();
        for key in self.rows.iter().map(|r| &r.key) {
            if self.get_live(key).is_some() && !keys.iter().any(|k| k == key) {
                keys.push(key.clone());
            }
        }
        keys.sort();
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_get_live_returns_value() {
        let mut s = Store::new();
        let rev = s.create(b"a", b"1", 0).unwrap();
        assert_eq!(rev, Some(1));
        let row = s.get_live(b"a").unwrap();
        assert_eq!(row.value, b"1");
        assert_eq!(row.create_revision, 1);
        assert_eq!(row.mod_revision, 1);
    }

    #[test]
    fn create_is_rejected_when_live_row_exists() {
        let mut s = Store::new();
        s.create(b"a", b"1", 0).unwrap();
        assert_eq!(s.create(b"a", b"2", 0).unwrap(), None);
        // value unchanged, revision did not advance from the no-op create
        assert_eq!(s.get_live(b"a").unwrap().value, b"1");
        assert_eq!(s.current_revision(), 1);
    }

    #[test]
    fn update_keeps_create_revision_and_advances_mod_revision() {
        let mut s = Store::new();
        s.create(b"a", b"1", 0).unwrap();
        let rev = s.update(b"a", b"2", 0).unwrap();
        assert_eq!(rev, Some(2));
        let row = s.get_live(b"a").unwrap();
        assert_eq!(row.create_revision, 1, "create_revision survives an update");
        assert_eq!(row.mod_revision, 2);
        assert_eq!(row.value, b"2");
    }

    #[test]
    fn update_absent_key_is_a_noop() {
        let mut s = Store::new();
        assert_eq!(s.update(b"ghost", b"x", 0).unwrap(), None);
        assert_eq!(s.current_revision(), 0);
    }

    #[test]
    fn delete_writes_a_tombstone_and_hides_the_key() {
        let mut s = Store::new();
        s.create(b"a", b"1", 0).unwrap();
        let rev = s.delete(b"a").unwrap();
        assert_eq!(rev, Some(2));
        assert!(s.get_live(b"a").is_none(), "deleted key has no live row");
        // tombstone row is present in the log
        let last = s.rows().last().unwrap();
        assert!(last.deleted);
        assert_eq!(last.mod_revision, 2);
    }

    #[test]
    fn delete_absent_key_is_a_noop() {
        let mut s = Store::new();
        assert_eq!(s.delete(b"ghost").unwrap(), None);
        assert_eq!(s.current_revision(), 0);
    }

    #[test]
    fn recreate_after_delete_starts_a_new_generation() {
        let mut s = Store::new();
        s.create(b"a", b"1", 0).unwrap(); // rev 1
        s.delete(b"a").unwrap(); //           rev 2 tombstone
        let rev = s.create(b"a", b"3", 0).unwrap(); // rev 3, new generation
        assert_eq!(rev, Some(3));
        let row = s.get_live(b"a").unwrap();
        assert_eq!(row.create_revision, 3, "recreated key gets a fresh create_revision");
        assert_eq!(row.mod_revision, 3);
    }

    #[test]
    fn revision_is_monotonic_across_mixed_ops() {
        let mut s = Store::new();
        s.create(b"a", b"1", 0).unwrap();
        s.put(b"b", b"2", 0).unwrap();
        s.update(b"a", b"1b", 0).unwrap();
        s.delete(b"b").unwrap();
        assert_eq!(s.current_revision(), 4);
        let revs: Vec<_> = s.rows().iter().map(|r| r.mod_revision).collect();
        assert_eq!(revs, vec![1, 2, 3, 4]);
        for w in revs.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn put_creates_or_updates() {
        let mut s = Store::new();
        assert_eq!(s.put(b"a", b"1", 0).unwrap(), 1);
        assert_eq!(s.put(b"a", b"2", 0).unwrap(), 2);
        let row = s.get_live(b"a").unwrap();
        assert_eq!(row.create_revision, 1);
        assert_eq!(row.mod_revision, 2);
        assert_eq!(row.value, b"2");
    }

    #[test]
    fn empty_key_is_rejected_on_every_mutation() {
        let mut s = Store::new();
        assert_eq!(s.create(b"", b"1", 0), Err(KineError::EmptyKey));
        assert_eq!(s.update(b"", b"1", 0), Err(KineError::EmptyKey));
        assert_eq!(s.put(b"", b"1", 0), Err(KineError::EmptyKey));
        assert_eq!(s.delete(b""), Err(KineError::EmptyKey));
    }

    #[test]
    fn current_state_view_lists_only_live_keys() {
        let mut s = Store::new();
        s.create(b"a", b"1", 0).unwrap();
        s.create(b"b", b"2", 0).unwrap();
        s.create(b"c", b"3", 0).unwrap();
        s.delete(b"b").unwrap();
        assert_eq!(s.live_keys(), vec![b"a".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn lease_is_stored_on_the_row() {
        let mut s = Store::new();
        s.create(b"a", b"1", 77).unwrap();
        assert_eq!(s.get_live(b"a").unwrap().lease, 77);
    }
}
