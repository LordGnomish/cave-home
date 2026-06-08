// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Watch semantics — the ordered change-event stream kine replays.
//!
//! An etcd `Watch` says "from revision `R+1` onward, tell me every change to
//! these keys". kine answers this by scanning its row log for rows with
//! `mod_revision > R` that fall in the watched key/range, and replaying them as
//! `PUT` / `DELETE` events in `mod_revision` order. Each non-tombstone row is a
//! `PUT`; each tombstone row is a `DELETE`.
//!
//! This module computes that event list as a pure function over [`Store`]. A
//! real implementation streams it; the *ordering and filtering* — the part that
//! must be correct — lives here and is tested hard.
//!
//! Reference: etcd `WatchRequest` (`start_revision`, key/range filter) and the
//! event types `PUT` / `DELETE`; kine `pkg/server` `Watch` which polls the same
//! row log. Behavioural reimplementation, Apache-2.0.

use crate::error::{KineError, Result};
use crate::range::{RangeEnd, RangeRequest};
use crate::revision::Revision;
use crate::store::Store;

/// The kind of change a [`WatchEvent`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// A create or update — a non-tombstone row.
    Put,
    /// A delete — a tombstone row.
    Delete,
}

/// One change event in a watch stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEvent {
    /// What happened.
    pub kind: EventKind,
    /// The affected key.
    pub key: Vec<u8>,
    /// The new value (empty for a `Delete`).
    pub value: Vec<u8>,
    /// The revision at which the change occurred (`mod_revision`).
    pub revision: Revision,
    /// The generation's create revision, so a consumer can detect re-creates.
    pub create_revision: Revision,
}

/// Compute the ordered event stream for `filter`, starting *after*
/// `start_revision`. The result is sorted by `revision` ascending — the exact
/// order etcd guarantees within a single watch.
///
/// `start_revision == 0` means "from the beginning of available history": every
/// retained change is replayed. A positive `start_revision` replays only
/// changes with `mod_revision > start_revision` (etcd watches start *after* the
/// given revision).
///
/// # Errors
/// * [`KineError::NegativeRevision`] if `start_revision` is negative.
/// * [`KineError::Compacted`] if `start_revision` is below the compacted floor —
///   etcd cannot replay history that compaction removed
///   (`ErrCompacted` on a watch).
/// * [`KineError::EmptyKey`] / [`KineError::InvalidRange`] from the filter.
pub fn watch(store: &Store, filter: &RangeRequest, start_revision: Revision) -> Result<Vec<WatchEvent>> {
    if start_revision < 0 {
        return Err(KineError::NegativeRevision { revision: start_revision });
    }
    let compacted = store.compacted_revision();
    // A watch from a revision strictly below the compacted floor cannot be
    // satisfied: the events between start_revision and the floor are gone.
    if start_revision != 0 && start_revision < compacted {
        return Err(KineError::Compacted { requested: start_revision, compacted });
    }
    validate_filter(filter)?;

    let mut events: Vec<WatchEvent> = store
        .rows()
        .iter()
        .filter(|r| r.mod_revision > start_revision && filter_contains(filter, &r.key))
        .map(|r| WatchEvent {
            kind: if r.deleted { EventKind::Delete } else { EventKind::Put },
            key: r.key.clone(),
            value: r.value.clone(),
            revision: r.mod_revision,
            create_revision: r.create_revision,
        })
        .collect();
    events.sort_by_key(|e| e.revision);
    Ok(events)
}

fn filter_contains(filter: &RangeRequest, candidate: &[u8]) -> bool {
    match &filter.end {
        RangeEnd::Single => candidate == filter.key.as_slice(),
        RangeEnd::Prefix => candidate.starts_with(&filter.key),
        RangeEnd::AllKeys => true,
        RangeEnd::Explicit(end) => {
            candidate >= filter.key.as_slice() && candidate < end.as_slice()
        }
    }
}

fn validate_filter(filter: &RangeRequest) -> Result<()> {
    match &filter.end {
        RangeEnd::AllKeys => Ok(()),
        RangeEnd::Single | RangeEnd::Prefix => {
            if filter.key.is_empty() {
                Err(KineError::EmptyKey)
            } else {
                Ok(())
            }
        }
        RangeEnd::Explicit(end) => {
            if filter.key.is_empty() {
                Err(KineError::EmptyKey)
            } else if end.as_slice() <= filter.key.as_slice() {
                Err(KineError::InvalidRange)
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::compact;

    fn history() -> Store {
        let mut s = Store::new();
        s.create(b"/a", b"1", 0).unwrap(); // rev 1 PUT /a
        s.create(b"/b", b"2", 0).unwrap(); // rev 2 PUT /b
        s.update(b"/a", b"1b", 0).unwrap(); // rev 3 PUT /a
        s.delete(b"/b").unwrap(); //           rev 4 DELETE /b
        s
    }

    #[test]
    fn watch_from_zero_replays_all_changes_in_order() {
        let s = history();
        let evs = watch(&s, &RangeRequest::all(), 0).unwrap();
        let revs: Vec<_> = evs.iter().map(|e| e.revision).collect();
        assert_eq!(revs, vec![1, 2, 3, 4], "events ordered by revision");
    }

    #[test]
    fn watch_classifies_put_and_delete() {
        let s = history();
        let evs = watch(&s, &RangeRequest::all(), 0).unwrap();
        assert_eq!(evs[0].kind, EventKind::Put);
        assert_eq!(evs[3].kind, EventKind::Delete);
        assert_eq!(evs[3].key, b"/b");
        assert!(evs[3].value.is_empty());
    }

    #[test]
    fn watch_starts_strictly_after_start_revision() {
        let s = history();
        // start at rev 2 -> only rev 3 and 4 replayed
        let evs = watch(&s, &RangeRequest::all(), 2).unwrap();
        let revs: Vec<_> = evs.iter().map(|e| e.revision).collect();
        assert_eq!(revs, vec![3, 4]);
    }

    #[test]
    fn watch_filters_to_a_single_key() {
        let s = history();
        let evs = watch(&s, &RangeRequest::key(b"/a"), 0).unwrap();
        let revs: Vec<_> = evs.iter().map(|e| e.revision).collect();
        assert_eq!(revs, vec![1, 3], "only /a events");
        assert!(evs.iter().all(|e| e.key == b"/a"));
    }

    #[test]
    fn watch_filters_to_a_prefix() {
        let mut s = Store::new();
        s.create(b"/ns/x", b"1", 0).unwrap(); // 1
        s.create(b"/other", b"2", 0).unwrap(); // 2
        s.create(b"/ns/y", b"3", 0).unwrap(); // 3
        let evs = watch(&s, &RangeRequest::prefix(b"/ns/"), 0).unwrap();
        let keys: Vec<_> = evs.iter().map(|e| e.key.clone()).collect();
        assert_eq!(keys, vec![b"/ns/x".to_vec(), b"/ns/y".to_vec()]);
    }

    #[test]
    fn watch_event_carries_create_revision_for_recreate_detection() {
        let mut s = Store::new();
        s.create(b"k", b"v", 0).unwrap(); // 1 create_rev 1
        s.delete(b"k").unwrap(); //          2
        s.create(b"k", b"v2", 0).unwrap(); // 3 create_rev 3
        let evs = watch(&s, &RangeRequest::key(b"k"), 0).unwrap();
        assert_eq!(evs[0].create_revision, 1);
        assert_eq!(evs[2].create_revision, 3, "recreate has a new create_revision");
    }

    #[test]
    fn watch_below_compacted_floor_is_rejected() {
        let mut s = history();
        compact(&mut s, 2).unwrap();
        let err = watch(&s, &RangeRequest::all(), 1).unwrap_err();
        assert_eq!(err, KineError::Compacted { requested: 1, compacted: 2 });
    }

    #[test]
    fn watch_at_or_above_floor_is_allowed_after_compaction() {
        let mut s = history();
        compact(&mut s, 2).unwrap();
        let evs = watch(&s, &RangeRequest::all(), 2).unwrap();
        let revs: Vec<_> = evs.iter().map(|e| e.revision).collect();
        assert_eq!(revs, vec![3, 4]);
    }

    #[test]
    fn watch_rejects_negative_start_revision() {
        let s = history();
        assert_eq!(
            watch(&s, &RangeRequest::all(), -1),
            Err(KineError::NegativeRevision { revision: -1 })
        );
    }

    #[test]
    fn watch_rejects_empty_key_filter() {
        let s = history();
        assert_eq!(watch(&s, &RangeRequest::key(b""), 0), Err(KineError::EmptyKey));
    }
}
