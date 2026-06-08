// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Range queries — the etcd `Range` RPC semantics kine emulates.
//!
//! etcd's `Range` selects keys in the half-open interval `[key, range_end)`:
//!
//! * `range_end` empty           → a point get of exactly `key`.
//! * `range_end == key+1` (the   → a *prefix* scan of everything under `key`
//!   "prefix successor")            (the convention `clientv3.WithPrefix` uses).
//! * any other `range_end`       → the literal interval `[key, range_end)`.
//! * the special `range_end = "\0"` with `key = "\0"` → the whole keyspace.
//!
//! On top of the interval, a request carries an optional `revision` (read the
//! historical state as of a past revision — etcd's `--rev`) and a `limit`.
//!
//! This module is the **SQL-shaped query plan as pure logic**: it walks the
//! append-only row log of [`Store`], picks the latest row per key *at or below*
//! the read revision, filters out tombstones and out-of-interval keys, sorts,
//! and applies the limit. A real driver would push the same plan down to SQL
//! (see [`crate::sql`]); the logic is identical, only the executor differs.
//!
//! Reference: etcd `etcdserverpb.RangeRequest` semantics and `clientv3`
//! prefix/range helpers; kine `pkg/server` `List`. Behavioural, Apache-2.0.

use crate::error::{KineError, Result};
use crate::revision::Revision;
use crate::store::{Row, Store};

/// The `range_end` bound of a [`RangeRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeEnd {
    /// No `range_end`: a point get of exactly the request key.
    Single,
    /// Prefix scan: every key that starts with the request key. Equivalent to
    /// etcd's `key+1` successor convention.
    Prefix,
    /// An explicit upper bound; the interval is `[key, end)`.
    Explicit(Vec<u8>),
    /// The whole keyspace (etcd's `key="\0", range_end="\0"`).
    AllKeys,
}

/// A resolved range query against the store.
#[derive(Debug, Clone)]
pub struct RangeRequest {
    /// The lower bound / point key. May be empty only when `end == AllKeys`.
    pub key: Vec<u8>,
    /// The upper-bound selector.
    pub end: RangeEnd,
    /// Historical read revision; `0` means "current".
    pub revision: Revision,
    /// Maximum number of results; `0` means unlimited. Negative is rejected.
    pub limit: i64,
}

impl RangeRequest {
    /// A point get of `key` at the current revision.
    #[must_use]
    pub fn key(key: &[u8]) -> Self {
        Self { key: key.to_vec(), end: RangeEnd::Single, revision: 0, limit: 0 }
    }

    /// A prefix scan under `prefix` at the current revision.
    #[must_use]
    pub fn prefix(prefix: &[u8]) -> Self {
        Self { key: prefix.to_vec(), end: RangeEnd::Prefix, revision: 0, limit: 0 }
    }

    /// An explicit `[key, end)` interval at the current revision.
    #[must_use]
    pub fn interval(key: &[u8], end: &[u8]) -> Self {
        Self {
            key: key.to_vec(),
            end: RangeEnd::Explicit(end.to_vec()),
            revision: 0,
            limit: 0,
        }
    }

    /// The whole keyspace at the current revision.
    #[must_use]
    pub const fn all() -> Self {
        Self { key: Vec::new(), end: RangeEnd::AllKeys, revision: 0, limit: 0 }
    }

    /// Builder: read at a historical revision.
    #[must_use]
    pub const fn at_revision(mut self, revision: Revision) -> Self {
        self.revision = revision;
        self
    }

    /// Builder: cap the result count.
    #[must_use]
    pub const fn with_limit(mut self, limit: i64) -> Self {
        self.limit = limit;
        self
    }

    /// Does `candidate` fall inside this request's key interval?
    fn contains(&self, candidate: &[u8]) -> bool {
        match &self.end {
            RangeEnd::Single => candidate == self.key.as_slice(),
            RangeEnd::Prefix => candidate.starts_with(&self.key),
            RangeEnd::AllKeys => true,
            RangeEnd::Explicit(end) => {
                candidate >= self.key.as_slice() && candidate < end.as_slice()
            }
        }
    }
}

/// The result of a range query: the selected current-state rows plus the store
/// header revision the read was served at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeResponse {
    /// Header revision (the store's current revision, or the historical
    /// revision the read was pinned to). Mirrors `RangeResponse.header.revision`.
    pub revision: Revision,
    /// Matching live key/value rows, sorted lexically by key.
    pub kvs: Vec<Row>,
    /// Whether more keys matched than `limit` allowed — etcd's `more` flag.
    pub more: bool,
    /// Count of matching keys *before* the limit was applied — etcd's `count`.
    pub count: i64,
}

/// Execute a [`RangeRequest`] against the store as pure logic.
///
/// # Errors
/// * [`KineError::EmptyKey`] — an empty `key` with a non-`AllKeys` end.
/// * [`KineError::InvalidRange`] — an explicit `end` that sorts at or before
///   `key` (selects nothing; etcd rejects it).
/// * [`KineError::NegativeLimit`] — a negative `limit`.
/// * [`KineError::NegativeRevision`] / [`KineError::FutureRevision`] /
///   [`KineError::Compacted`] — via revision resolution.
pub fn execute(store: &Store, req: &RangeRequest) -> Result<RangeResponse> {
    validate(req)?;

    // Resolve the read revision, then guard it against the compacted floor.
    let read_rev = store.clock().resolve_read(req.revision)?;
    let compacted = store.compacted_revision();
    if req.revision != 0 && req.revision < compacted {
        return Err(KineError::Compacted { requested: req.revision, compacted });
    }

    // Build the current-state view AS OF read_rev: for each key, the latest row
    // with mod_revision <= read_rev. This is the SQL "max(mod_revision) group
    // by key" plan, filtered to the read revision.
    let mut latest: Vec<&Row> = Vec::new();
    for key in distinct_keys_in_order(store) {
        if let Some(row) = latest_at(store, key, read_rev) {
            if row.is_live() && req.contains(&row.key) {
                latest.push(row);
            }
        }
    }
    latest.sort_by(|a, b| a.key.cmp(&b.key));

    let count = latest.len() as i64;
    let (kvs, more) = if req.limit > 0 && count > req.limit {
        let take = usize::try_from(req.limit).unwrap_or(usize::MAX);
        (latest[..take].iter().map(|r| (*r).clone()).collect(), true)
    } else {
        (latest.iter().map(|r| (*r).clone()).collect(), false)
    };

    Ok(RangeResponse { revision: read_rev, kvs, more, count })
}

fn validate(req: &RangeRequest) -> Result<()> {
    if req.limit < 0 {
        return Err(KineError::NegativeLimit { limit: req.limit });
    }
    match &req.end {
        RangeEnd::AllKeys => Ok(()),
        RangeEnd::Single | RangeEnd::Prefix => {
            if req.key.is_empty() {
                Err(KineError::EmptyKey)
            } else {
                Ok(())
            }
        }
        RangeEnd::Explicit(end) => {
            if req.key.is_empty() {
                return Err(KineError::EmptyKey);
            }
            if end.as_slice() <= req.key.as_slice() {
                return Err(KineError::InvalidRange);
            }
            Ok(())
        }
    }
}

/// Distinct keys in first-seen order across the whole log (live or not). Used
/// as the iteration set; membership/liveness is decided per read revision.
fn distinct_keys_in_order(store: &Store) -> Vec<&[u8]> {
    let mut keys: Vec<&[u8]> = Vec::new();
    for row in store.rows() {
        let k = row.key.as_slice();
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

/// The latest row for `key` whose `mod_revision <= read_rev` (may be a
/// tombstone). `None` if the key did not exist by `read_rev`.
fn latest_at<'a>(store: &'a Store, key: &[u8], read_rev: Revision) -> Option<&'a Row> {
    store
        .rows()
        .iter()
        .rev()
        .find(|r| r.key == key && r.mod_revision <= read_rev)
}

/// Compute the prefix successor of `prefix`.
///
/// This is the smallest key strictly greater than every key starting with
/// `prefix` — what `clientv3.WithPrefix` sends as `range_end`. Increments the
/// last byte; if all trailing bytes are `0xFF`, returns `"\0"` meaning "to the
/// end of the keyspace".
///
/// Exposed so a real driver can reproduce the exact `range_end` etcd clients
/// send, and so tests can assert the convention.
#[must_use]
pub fn prefix_successor(prefix: &[u8]) -> Vec<u8> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.last_mut() {
        if *last < 0xFF {
            *last += 1;
            return end;
        }
        end.pop();
    }
    // All 0xFF (or empty): no finite successor → whole keyspace from here.
    vec![0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> Store {
        let mut s = Store::new();
        s.create(b"/reg/a", b"1", 0).unwrap();
        s.create(b"/reg/b", b"2", 0).unwrap();
        s.create(b"/reg/c", b"3", 0).unwrap();
        s.create(b"/other/x", b"9", 0).unwrap();
        s
    }

    #[test]
    fn point_get_returns_single_key() {
        let s = seeded();
        let resp = execute(&s, &RangeRequest::key(b"/reg/b")).unwrap();
        assert_eq!(resp.kvs.len(), 1);
        assert_eq!(resp.kvs[0].value, b"2");
        assert_eq!(resp.count, 1);
        assert!(!resp.more);
    }

    #[test]
    fn point_get_missing_key_is_empty() {
        let s = seeded();
        let resp = execute(&s, &RangeRequest::key(b"/reg/zzz")).unwrap();
        assert!(resp.kvs.is_empty());
        assert_eq!(resp.count, 0);
    }

    #[test]
    fn prefix_scan_selects_only_the_subtree_sorted() {
        let s = seeded();
        let resp = execute(&s, &RangeRequest::prefix(b"/reg/")).unwrap();
        let keys: Vec<_> = resp.kvs.iter().map(|r| r.key.clone()).collect();
        assert_eq!(
            keys,
            vec![b"/reg/a".to_vec(), b"/reg/b".to_vec(), b"/reg/c".to_vec()]
        );
        assert_eq!(resp.count, 3);
    }

    #[test]
    fn prefix_scan_excludes_sibling_subtrees() {
        let s = seeded();
        let resp = execute(&s, &RangeRequest::prefix(b"/reg/")).unwrap();
        assert!(!resp.kvs.iter().any(|r| r.key == b"/other/x"));
    }

    #[test]
    fn explicit_interval_is_half_open() {
        let s = seeded();
        // [/reg/a, /reg/c) -> a, b but NOT c (exclusive end)
        let resp = execute(&s, &RangeRequest::interval(b"/reg/a", b"/reg/c")).unwrap();
        let keys: Vec<_> = resp.kvs.iter().map(|r| r.key.clone()).collect();
        assert_eq!(keys, vec![b"/reg/a".to_vec(), b"/reg/b".to_vec()]);
    }

    #[test]
    fn all_keys_returns_everything_live() {
        let s = seeded();
        let resp = execute(&s, &RangeRequest::all()).unwrap();
        assert_eq!(resp.count, 4);
    }

    #[test]
    fn deleted_keys_are_absent_from_current_view() {
        let mut s = seeded();
        s.delete(b"/reg/b").unwrap();
        let resp = execute(&s, &RangeRequest::prefix(b"/reg/")).unwrap();
        let keys: Vec<_> = resp.kvs.iter().map(|r| r.key.clone()).collect();
        assert_eq!(keys, vec![b"/reg/a".to_vec(), b"/reg/c".to_vec()]);
    }

    #[test]
    fn historical_read_sees_old_value() {
        let mut s = Store::new();
        s.create(b"k", b"v1", 0).unwrap(); // rev 1
        s.update(b"k", b"v2", 0).unwrap(); // rev 2
        let now = execute(&s, &RangeRequest::key(b"k")).unwrap();
        assert_eq!(now.kvs[0].value, b"v2");
        let past = execute(&s, &RangeRequest::key(b"k").at_revision(1)).unwrap();
        assert_eq!(past.kvs[0].value, b"v1");
        assert_eq!(past.revision, 1);
    }

    #[test]
    fn historical_read_before_create_is_empty() {
        let mut s = Store::new();
        s.create(b"first", b"x", 0).unwrap(); // rev 1
        s.create(b"k", b"v", 0).unwrap(); //      rev 2
        let past = execute(&s, &RangeRequest::key(b"k").at_revision(1)).unwrap();
        assert!(past.kvs.is_empty(), "k did not exist at rev 1");
    }

    #[test]
    fn historical_read_sees_key_as_live_before_its_delete() {
        let mut s = Store::new();
        s.create(b"k", b"v", 0).unwrap(); // rev 1
        s.delete(b"k").unwrap(); //          rev 2
        let past = execute(&s, &RangeRequest::key(b"k").at_revision(1)).unwrap();
        assert_eq!(past.kvs.len(), 1);
        assert_eq!(past.kvs[0].value, b"v");
        let now = execute(&s, &RangeRequest::key(b"k")).unwrap();
        assert!(now.kvs.is_empty());
    }

    #[test]
    fn limit_truncates_and_sets_more() {
        let s = seeded();
        let resp = execute(&s, &RangeRequest::prefix(b"/reg/").with_limit(2)).unwrap();
        assert_eq!(resp.kvs.len(), 2);
        assert!(resp.more);
        assert_eq!(resp.count, 3, "count reflects pre-limit total");
        // limit respects sort order: first two keys
        assert_eq!(resp.kvs[0].key, b"/reg/a");
        assert_eq!(resp.kvs[1].key, b"/reg/b");
    }

    #[test]
    fn limit_at_or_above_count_clears_more() {
        let s = seeded();
        let resp = execute(&s, &RangeRequest::prefix(b"/reg/").with_limit(3)).unwrap();
        assert_eq!(resp.kvs.len(), 3);
        assert!(!resp.more);
    }

    #[test]
    fn negative_limit_is_rejected() {
        let s = seeded();
        let req = RangeRequest::key(b"/reg/a").with_limit(-1);
        assert_eq!(execute(&s, &req), Err(KineError::NegativeLimit { limit: -1 }));
    }

    #[test]
    fn empty_key_point_get_is_rejected() {
        let s = seeded();
        assert_eq!(execute(&s, &RangeRequest::key(b"")), Err(KineError::EmptyKey));
    }

    #[test]
    fn inverted_interval_is_rejected() {
        let s = seeded();
        let req = RangeRequest::interval(b"/reg/c", b"/reg/a");
        assert_eq!(execute(&s, &req), Err(KineError::InvalidRange));
    }

    #[test]
    fn equal_interval_bounds_are_rejected() {
        let s = seeded();
        let req = RangeRequest::interval(b"/reg/a", b"/reg/a");
        assert_eq!(execute(&s, &req), Err(KineError::InvalidRange));
    }

    #[test]
    fn prefix_successor_increments_last_byte() {
        assert_eq!(prefix_successor(b"/reg/"), b"/reg0".to_vec());
        assert_eq!(prefix_successor(b"ab"), b"ac".to_vec());
    }

    #[test]
    fn prefix_successor_carries_over_trailing_ff() {
        assert_eq!(prefix_successor(&[0x61, 0xFF]), vec![0x62]);
        assert_eq!(prefix_successor(&[0xFF, 0xFF]), vec![0x00]);
    }
}
