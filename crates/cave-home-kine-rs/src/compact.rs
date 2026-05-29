// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Compaction — dropping superseded history, and the "compacted" read guard.
//!
//! etcd (and kine on top of SQL) keeps every historical row until a `Compact`
//! call sets a floor revision. Compaction at revision `C`:
//!
//! * **removes** every row with `mod_revision <= C` that is *not* the latest
//!   live row of its key — i.e. superseded values and old tombstones whose key
//!   has since been re-handled;
//! * **removes** tombstone rows at or below `C` entirely once nothing needs
//!   them (a deleted key with no later generation simply disappears);
//! * **keeps** the single surviving live row per key even if its `mod_revision`
//!   is `<= C`, so the current state is never destroyed;
//! * **moves the compacted floor** to `C`, after which any *historical* read
//!   below `C` is rejected with [`KineError::Compacted`] — etcd's famous
//!   `"mvcc: required revision has been compacted"`.
//!
//! kine implements this as a periodic SQL `DELETE` of superseded rows plus a
//! recorded compact revision; the SQL is modelled in [`crate::sql`]. Here it is
//! pure logic over the row log.
//!
//! Reference: etcd `Compact` / MVCC compaction and kine `pkg/server`
//! `Compact` + the generic backend's `DELETE FROM kine WHERE ...`.
//! Behavioural reimplementation, Apache-2.0.

use crate::error::{KineError, Result};
use crate::revision::Revision;
use crate::store::{Row, Store};

/// Outcome of a compaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactReport {
    /// The new compacted floor.
    pub compacted: Revision,
    /// How many rows were physically removed.
    pub removed: usize,
    /// How many rows remain after compaction.
    pub remaining: usize,
}

/// Compact the store up to and including `target`.
///
/// # Errors
/// * [`KineError::NegativeRevision`] if `target` is negative.
/// * [`KineError::CompactFutureRevision`] if `target` exceeds the store's
///   current revision.
/// * [`KineError::CompactionNotForward`] if `target` is at or below the existing
///   compacted floor (etcd rejects a non-advancing compact).
pub fn compact(store: &mut Store, target: Revision) -> Result<CompactReport> {
    if target < 0 {
        return Err(KineError::NegativeRevision { revision: target });
    }
    let current = store.current_revision();
    if target > current {
        return Err(KineError::CompactFutureRevision { requested: target, current });
    }
    let floor = store.compacted_revision();
    if target <= floor {
        return Err(KineError::CompactionNotForward { requested: target, current: floor });
    }

    let before = store.rows().len();

    // The single latest row per key (live or tombstone) is the survivor that
    // protects the current state. We must never drop it, even at/below target.
    let survivors = latest_row_revision_per_key(store.rows());

    let kept: Vec<Row> = store
        .rows()
        .iter()
        .filter(|r| {
            let is_latest = survivors
                .iter()
                .any(|(k, rev)| k.as_slice() == r.key.as_slice() && *rev == r.mod_revision);
            if r.mod_revision > target {
                // Above the floor: always retained.
                true
            } else if is_latest {
                // At/below the floor but the current state of a still-live key:
                // keep it ONLY if it is live. A latest-row tombstone at/below
                // the floor is fully compactable (the key is gone for good).
                r.is_live()
            } else {
                // Superseded history at/below the floor: drop.
                false
            }
        })
        .cloned()
        .collect();

    let removed = before - kept.len();
    let remaining = kept.len();
    store.install_compacted(kept, target);
    Ok(CompactReport { compacted: target, removed, remaining })
}

/// `(key, mod_revision)` of the latest row for each distinct key.
fn latest_row_revision_per_key(rows: &[Row]) -> Vec<(Vec<u8>, Revision)> {
    let mut out: Vec<(Vec<u8>, Revision)> = Vec::new();
    for row in rows {
        match out.iter_mut().find(|(k, _)| k.as_slice() == row.key.as_slice()) {
            Some((_, rev)) if *rev < row.mod_revision => *rev = row.mod_revision,
            Some(_) => {}
            None => out.push((row.key.clone(), row.mod_revision)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::range::{execute, RangeRequest};

    #[test]
    fn compaction_drops_superseded_values_but_keeps_current() {
        let mut s = Store::new();
        s.create(b"k", b"v1", 0).unwrap(); // rev 1
        s.update(b"k", b"v2", 0).unwrap(); // rev 2
        s.update(b"k", b"v3", 0).unwrap(); // rev 3
        let report = compact(&mut s, 2).unwrap();
        // rev1 and rev2 superseded by rev3 -> rev1 (<=2, not latest) dropped.
        assert_eq!(report.compacted, 2);
        assert!(report.removed >= 1);
        // current value survives
        let resp = execute(&s, &RangeRequest::key(b"k")).unwrap();
        assert_eq!(resp.kvs[0].value, b"v3");
    }

    #[test]
    fn compaction_keeps_a_live_row_even_if_below_floor() {
        let mut s = Store::new();
        s.create(b"k", b"v1", 0).unwrap(); // rev 1, never updated again
        s.create(b"other", b"o", 0).unwrap(); // rev 2
        compact(&mut s, 2).unwrap();
        // k's only row is rev1 (<= floor 2) but it is live -> must survive.
        let resp = execute(&s, &RangeRequest::key(b"k")).unwrap();
        assert_eq!(resp.kvs[0].value, b"v1");
    }

    #[test]
    fn compaction_removes_tombstone_at_or_below_floor() {
        let mut s = Store::new();
        s.create(b"k", b"v1", 0).unwrap(); // rev 1
        s.delete(b"k").unwrap(); //           rev 2 tombstone (latest row)
        let report = compact(&mut s, 2).unwrap();
        // Both the value and the tombstone are <= 2; the key is gone.
        assert_eq!(report.remaining, 0);
        let resp = execute(&s, &RangeRequest::key(b"k")).unwrap();
        assert!(resp.kvs.is_empty());
    }

    #[test]
    fn historical_read_below_compacted_floor_is_rejected() {
        let mut s = Store::new();
        s.create(b"k", b"v1", 0).unwrap(); // 1
        s.update(b"k", b"v2", 0).unwrap(); // 2
        s.update(b"k", b"v3", 0).unwrap(); // 3
        compact(&mut s, 2).unwrap();
        let err = execute(&s, &RangeRequest::key(b"k").at_revision(1)).unwrap_err();
        assert_eq!(err, KineError::Compacted { requested: 1, compacted: 2 });
    }

    #[test]
    fn read_at_or_above_floor_still_works_after_compaction() {
        let mut s = Store::new();
        s.create(b"k", b"v1", 0).unwrap();
        s.update(b"k", b"v2", 0).unwrap();
        s.update(b"k", b"v3", 0).unwrap();
        compact(&mut s, 2).unwrap();
        // current read fine
        assert_eq!(execute(&s, &RangeRequest::key(b"k")).unwrap().kvs[0].value, b"v3");
        // read at exactly the floor fine
        assert!(execute(&s, &RangeRequest::key(b"k").at_revision(2)).is_ok());
    }

    #[test]
    fn compaction_must_move_forward() {
        let mut s = Store::new();
        s.create(b"k", b"v", 0).unwrap();
        s.update(b"k", b"v2", 0).unwrap();
        compact(&mut s, 2).unwrap();
        let err = compact(&mut s, 2).unwrap_err();
        assert_eq!(err, KineError::CompactionNotForward { requested: 2, current: 2 });
    }

    #[test]
    fn compaction_rejects_future_revision() {
        let mut s = Store::new();
        s.create(b"k", b"v", 0).unwrap();
        let err = compact(&mut s, 50).unwrap_err();
        assert_eq!(err, KineError::CompactFutureRevision { requested: 50, current: 1 });
    }

    #[test]
    fn compaction_rejects_negative_revision() {
        let mut s = Store::new();
        s.create(b"k", b"v", 0).unwrap();
        assert_eq!(compact(&mut s, -1), Err(KineError::NegativeRevision { revision: -1 }));
    }

    #[test]
    fn current_revision_is_unaffected_by_compaction() {
        let mut s = Store::new();
        s.create(b"k", b"v", 0).unwrap();
        s.update(b"k", b"v2", 0).unwrap();
        s.update(b"k", b"v3", 0).unwrap();
        compact(&mut s, 2).unwrap();
        assert_eq!(s.current_revision(), 3, "compaction never rewinds the clock");
    }
}
