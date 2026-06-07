// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The real `SQLite` storage backend — kine's single-binary default datastore.
//!
//! This is the actual driver, not a model: it opens a real `SQLite` database
//! (embedded, bundled — no server, one file), runs the kine DDL from
//! [`crate::dialect`], and executes every etcd-MVCC operation as live SQL
//! against it. The append-only row log lives in the `kine` table; the auto-
//! increment `id` is the global revision; the latest-row-per-key join is pushed
//! down to `SQLite`. The pure semantics proven in [`crate::store`] /
//! [`crate::range`] / [`crate::watch`] / [`crate::compact`] are here realised on
//! real storage.
//!
//! Keys are k8s/etcd registry paths and are stored in the `name TEXT` column as
//! UTF-8, exactly as kine does (kine keys are always valid UTF-8 strings); a
//! non-UTF-8 key is rejected. Prefix scans use `name LIKE 'prefix%'`, the
//! convention kine relies on (registry keys never contain `%`/`_`).
//!
//! Reference: `k3s-io/kine` `pkg/drivers/generic/generic.go` (the SQL) and
//! `pkg/logstructured/sqllog` (the create/update/delete/list/after/compact
//! flow). Faithful behavioural port on a real driver, Apache-2.0.

#![cfg(feature = "sqlite")]

use std::path::Path;

use crate::error::KineError;
use crate::range::{RangeRequest, RangeResponse};
use crate::watch::EventKind;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn store() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    fn keys(resp: &RangeResponse) -> Vec<Vec<u8>> {
        resp.kvs.iter().map(|r| r.key.clone()).collect()
    }

    #[test]
    fn create_then_range_returns_value_at_revision_one() {
        let mut s = store();
        assert_eq!(s.create(b"/reg/a", b"1", 0).unwrap(), Some(1));
        let resp = s.range(&RangeRequest::key(b"/reg/a")).unwrap();
        assert_eq!(resp.kvs.len(), 1);
        assert_eq!(resp.kvs[0].value, b"1");
        assert_eq!(resp.kvs[0].create_revision, 1);
        assert_eq!(resp.kvs[0].mod_revision, 1);
    }

    #[test]
    fn create_is_rejected_when_live_row_exists() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap();
        assert_eq!(s.create(b"a", b"2", 0).unwrap(), None);
        assert_eq!(s.range(&RangeRequest::key(b"a")).unwrap().kvs[0].value, b"1");
        assert_eq!(s.current_revision().unwrap(), 1);
    }

    #[test]
    fn update_keeps_create_revision_and_advances_mod_revision() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap();
        assert_eq!(s.update(b"a", b"2", 0).unwrap(), Some(2));
        let row = &s.range(&RangeRequest::key(b"a")).unwrap().kvs[0];
        assert_eq!(row.create_revision, 1);
        assert_eq!(row.mod_revision, 2);
        assert_eq!(row.value, b"2");
    }

    #[test]
    fn update_absent_key_is_a_noop() {
        let mut s = store();
        assert_eq!(s.update(b"ghost", b"x", 0).unwrap(), None);
        assert_eq!(s.current_revision().unwrap(), 0);
    }

    #[test]
    fn put_creates_then_updates() {
        let mut s = store();
        assert_eq!(s.put(b"a", b"1", 0).unwrap(), 1);
        assert_eq!(s.put(b"a", b"2", 0).unwrap(), 2);
        let row = &s.range(&RangeRequest::key(b"a")).unwrap().kvs[0];
        assert_eq!(row.create_revision, 1);
        assert_eq!(row.mod_revision, 2);
        assert_eq!(row.value, b"2");
    }

    #[test]
    fn delete_tombstones_and_hides_the_key() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap();
        assert_eq!(s.delete(b"a").unwrap(), Some(2));
        assert!(s.range(&RangeRequest::key(b"a")).unwrap().kvs.is_empty());
    }

    #[test]
    fn delete_absent_key_is_a_noop() {
        let mut s = store();
        assert_eq!(s.delete(b"ghost").unwrap(), None);
        assert_eq!(s.current_revision().unwrap(), 0);
    }

    #[test]
    fn recreate_after_delete_starts_a_new_generation() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap(); // 1
        s.delete(b"a").unwrap(); //          2
        assert_eq!(s.create(b"a", b"3", 0).unwrap(), Some(3));
        let row = &s.range(&RangeRequest::key(b"a")).unwrap().kvs[0];
        assert_eq!(row.create_revision, 3, "fresh generation");
        assert_eq!(row.mod_revision, 3);
        assert_eq!(row.value, b"3");
    }

    #[test]
    fn revision_is_monotonic_across_mixed_ops() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap();
        s.put(b"b", b"2", 0).unwrap();
        s.update(b"a", b"1b", 0).unwrap();
        s.delete(b"b").unwrap();
        assert_eq!(s.current_revision().unwrap(), 4);
    }

    #[test]
    fn empty_key_is_rejected_on_every_mutation() {
        let mut s = store();
        assert_eq!(s.create(b"", b"1", 0), Err(KineError::EmptyKey));
        assert_eq!(s.update(b"", b"1", 0), Err(KineError::EmptyKey));
        assert_eq!(s.put(b"", b"1", 0), Err(KineError::EmptyKey));
        assert_eq!(s.delete(b""), Err(KineError::EmptyKey));
    }

    #[test]
    fn lease_is_stored_on_the_row() {
        let mut s = store();
        s.create(b"a", b"1", 77).unwrap();
        assert_eq!(s.range(&RangeRequest::key(b"a")).unwrap().kvs[0].lease, 77);
    }

    fn seeded() -> SqliteStore {
        let mut s = store();
        s.create(b"/reg/a", b"1", 0).unwrap();
        s.create(b"/reg/b", b"2", 0).unwrap();
        s.create(b"/reg/c", b"3", 0).unwrap();
        s.create(b"/other/x", b"9", 0).unwrap();
        s
    }

    #[test]
    fn point_get_missing_key_is_empty() {
        let s = seeded();
        let resp = s.range(&RangeRequest::key(b"/reg/zzz")).unwrap();
        assert!(resp.kvs.is_empty());
        assert_eq!(resp.count, 0);
    }

    #[test]
    fn prefix_scan_selects_only_the_subtree_sorted() {
        let s = seeded();
        let resp = s.range(&RangeRequest::prefix(b"/reg/")).unwrap();
        assert_eq!(
            keys(&resp),
            vec![b"/reg/a".to_vec(), b"/reg/b".to_vec(), b"/reg/c".to_vec()]
        );
        assert_eq!(resp.count, 3);
    }

    #[test]
    fn prefix_scan_excludes_sibling_subtrees() {
        let s = seeded();
        let resp = s.range(&RangeRequest::prefix(b"/reg/")).unwrap();
        assert!(!resp.kvs.iter().any(|r| r.key == b"/other/x"));
    }

    #[test]
    fn explicit_interval_is_half_open() {
        let s = seeded();
        let resp = s.range(&RangeRequest::interval(b"/reg/a", b"/reg/c")).unwrap();
        assert_eq!(keys(&resp), vec![b"/reg/a".to_vec(), b"/reg/b".to_vec()]);
    }

    #[test]
    fn all_keys_returns_everything_live() {
        let s = seeded();
        assert_eq!(s.range(&RangeRequest::all()).unwrap().count, 4);
    }

    #[test]
    fn deleted_keys_are_absent_from_current_view() {
        let mut s = seeded();
        s.delete(b"/reg/b").unwrap();
        let resp = s.range(&RangeRequest::prefix(b"/reg/")).unwrap();
        assert_eq!(keys(&resp), vec![b"/reg/a".to_vec(), b"/reg/c".to_vec()]);
    }

    #[test]
    fn historical_read_sees_old_value() {
        let mut s = store();
        s.create(b"k", b"v1", 0).unwrap(); // 1
        s.update(b"k", b"v2", 0).unwrap(); // 2
        assert_eq!(s.range(&RangeRequest::key(b"k")).unwrap().kvs[0].value, b"v2");
        let past = s.range(&RangeRequest::key(b"k").at_revision(1)).unwrap();
        assert_eq!(past.kvs[0].value, b"v1");
        assert_eq!(past.revision, 1);
    }

    #[test]
    fn historical_read_before_create_is_empty() {
        let mut s = store();
        s.create(b"first", b"x", 0).unwrap(); // 1
        s.create(b"k", b"v", 0).unwrap(); //      2
        let past = s.range(&RangeRequest::key(b"k").at_revision(1)).unwrap();
        assert!(past.kvs.is_empty());
    }

    #[test]
    fn limit_truncates_and_sets_more_with_full_count() {
        let s = seeded();
        let resp = s.range(&RangeRequest::prefix(b"/reg/").with_limit(2)).unwrap();
        assert_eq!(resp.kvs.len(), 2);
        assert!(resp.more);
        assert_eq!(resp.count, 3);
        assert_eq!(resp.kvs[0].key, b"/reg/a");
        assert_eq!(resp.kvs[1].key, b"/reg/b");
    }

    #[test]
    fn future_revision_read_is_rejected() {
        let mut s = store();
        s.create(b"k", b"v", 0).unwrap();
        assert_eq!(
            s.range(&RangeRequest::key(b"k").at_revision(99)),
            Err(KineError::FutureRevision { requested: 99, current: 1 })
        );
    }

    fn history() -> SqliteStore {
        let mut s = store();
        s.create(b"/a", b"1", 0).unwrap(); // 1 PUT
        s.create(b"/b", b"2", 0).unwrap(); // 2 PUT
        s.update(b"/a", b"1b", 0).unwrap(); // 3 PUT
        s.delete(b"/b").unwrap(); //           4 DELETE
        s
    }

    #[test]
    fn watch_from_zero_replays_all_changes_in_order() {
        let s = history();
        let evs = s.watch_after(&RangeRequest::all(), 0).unwrap();
        let revs: Vec<_> = evs.iter().map(|e| e.revision).collect();
        assert_eq!(revs, vec![1, 2, 3, 4]);
    }

    #[test]
    fn watch_classifies_put_and_delete() {
        let s = history();
        let evs = s.watch_after(&RangeRequest::all(), 0).unwrap();
        assert_eq!(evs[0].kind, EventKind::Put);
        assert_eq!(evs[3].kind, EventKind::Delete);
        assert_eq!(evs[3].key, b"/b");
        assert!(evs[3].value.is_empty());
    }

    #[test]
    fn watch_starts_strictly_after_start_revision() {
        let s = history();
        let evs = s.watch_after(&RangeRequest::all(), 2).unwrap();
        let revs: Vec<_> = evs.iter().map(|e| e.revision).collect();
        assert_eq!(revs, vec![3, 4]);
    }

    #[test]
    fn watch_filters_to_a_prefix() {
        let mut s = store();
        s.create(b"/ns/x", b"1", 0).unwrap();
        s.create(b"/other", b"2", 0).unwrap();
        s.create(b"/ns/y", b"3", 0).unwrap();
        let evs = s.watch_after(&RangeRequest::prefix(b"/ns/"), 0).unwrap();
        let ks: Vec<_> = evs.iter().map(|e| e.key.clone()).collect();
        assert_eq!(ks, vec![b"/ns/x".to_vec(), b"/ns/y".to_vec()]);
    }

    #[test]
    fn compaction_drops_superseded_values_but_keeps_current() {
        let mut s = store();
        s.create(b"k", b"v1", 0).unwrap(); // 1
        s.update(b"k", b"v2", 0).unwrap(); // 2
        s.update(b"k", b"v3", 0).unwrap(); // 3
        let report = s.compact(2).unwrap();
        assert_eq!(report.compacted, 2);
        assert!(report.removed >= 1);
        assert_eq!(s.range(&RangeRequest::key(b"k")).unwrap().kvs[0].value, b"v3");
    }

    #[test]
    fn compaction_removes_tombstone_at_or_below_floor() {
        let mut s = store();
        s.create(b"k", b"v1", 0).unwrap(); // 1
        s.delete(b"k").unwrap(); //           2 tombstone
        s.compact(2).unwrap();
        assert!(s.range(&RangeRequest::key(b"k")).unwrap().kvs.is_empty());
        // physically gone
        let n: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM kine WHERE name = 'k'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn historical_read_below_compacted_floor_is_rejected() {
        let mut s = store();
        s.create(b"k", b"v1", 0).unwrap();
        s.update(b"k", b"v2", 0).unwrap();
        s.update(b"k", b"v3", 0).unwrap();
        s.compact(2).unwrap();
        assert_eq!(
            s.range(&RangeRequest::key(b"k").at_revision(1)),
            Err(KineError::Compacted { requested: 1, compacted: 2 })
        );
    }

    #[test]
    fn compaction_must_move_forward() {
        let mut s = store();
        s.create(b"k", b"v", 0).unwrap();
        s.update(b"k", b"v2", 0).unwrap();
        s.compact(2).unwrap();
        assert_eq!(
            s.compact(2),
            Err(KineError::CompactionNotForward { requested: 2, current: 2 })
        );
    }

    #[test]
    fn compaction_rejects_future_revision() {
        let mut s = store();
        s.create(b"k", b"v", 0).unwrap();
        assert_eq!(
            s.compact(50),
            Err(KineError::CompactFutureRevision { requested: 50, current: 1 })
        );
    }

    #[test]
    fn data_survives_close_and_reopen_on_disk() {
        // The anti-stub proof: write through one handle, drop it, reopen the
        // SAME file, and read the data back from real storage.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("kine-persist-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);

        {
            let mut s = SqliteStore::open(&path).unwrap();
            s.create(b"/reg/persisted", b"durable", 0).unwrap();
            s.update(b"/reg/persisted", b"durable2", 0).unwrap();
            assert_eq!(s.current_revision().unwrap(), 2);
        } // handle dropped, connection closed

        let s = SqliteStore::open(&path).unwrap();
        let resp = s.range(&RangeRequest::key(b"/reg/persisted")).unwrap();
        assert_eq!(resp.kvs[0].value, b"durable2");
        assert_eq!(resp.kvs[0].create_revision, 1);
        assert_eq!(resp.kvs[0].mod_revision, 2);
        assert_eq!(s.current_revision().unwrap(), 2);

        drop(s);
        let _ = std::fs::remove_file(&path);
    }
}
