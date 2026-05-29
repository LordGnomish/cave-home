// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Lease / TTL model — time-bound keys and the expiry decision.
//!
//! etcd leases let a client attach keys to a lease id with a TTL; when the
//! lease is not kept alive past its TTL the lease expires and **every key
//! attached to it is deleted** in one revision. kine carries the `lease` column
//! on each row (already modelled in [`crate::store::Row`]) and a lease table
//! mapping `lease id -> (ttl, granted_at)`.
//!
//! This module owns the lease table and the *expiry decision*: given a caller-
//! supplied "now", which leases have expired, and applying that by deleting
//! their keys from the store. Time is injected (the caller supplies `now`) so
//! the logic is pure and deterministic — no clock dependency, fully testable.
//!
//! Reference: etcd `LeaseGrant` / `LeaseRevoke` / TTL expiry and kine's lease
//! handling in `pkg/server`. Behavioural reimplementation, Apache-2.0.

use std::collections::BTreeMap;

use crate::error::{KineError, Result};
use crate::store::Store;

/// A monotonic timestamp in whole seconds, supplied by the caller. Using a
/// plain `i64` keeps this module pure and free of a system-clock dependency.
pub type UnixSeconds = i64;

/// One granted lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lease {
    /// The lease id (never `0`).
    pub id: i64,
    /// Time-to-live in seconds.
    pub ttl_seconds: i64,
    /// When the lease was granted (or last renewed).
    pub granted_at: UnixSeconds,
}

impl Lease {
    /// The absolute time at which this lease expires.
    #[must_use]
    pub const fn expires_at(&self) -> UnixSeconds {
        self.granted_at.saturating_add(self.ttl_seconds)
    }

    /// Has the lease expired as of `now`? Expiry is inclusive of the boundary —
    /// at exactly `expires_at` the lease is considered expired, matching etcd's
    /// "TTL elapsed" semantics.
    #[must_use]
    pub const fn is_expired(&self, now: UnixSeconds) -> bool {
        now >= self.expires_at()
    }
}

/// The lease table: `lease id -> Lease`.
#[derive(Debug, Clone, Default)]
pub struct LeaseTable {
    leases: BTreeMap<i64, Lease>,
}

impl LeaseTable {
    /// An empty lease table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant (or renew) a lease. Renewing an existing id resets its
    /// `granted_at` and `ttl`, mirroring `LeaseKeepAlive` extending the TTL.
    ///
    /// # Errors
    /// * [`KineError::InvalidLeaseId`] if `id == 0` (reserved for "no lease").
    /// * [`KineError::InvalidTtl`] if `ttl_seconds <= 0`.
    pub fn grant(&mut self, id: i64, ttl_seconds: i64, now: UnixSeconds) -> Result<Lease> {
        if id == 0 {
            return Err(KineError::InvalidLeaseId);
        }
        if ttl_seconds <= 0 {
            return Err(KineError::InvalidTtl { ttl_seconds });
        }
        let lease = Lease { id, ttl_seconds, granted_at: now };
        self.leases.insert(id, lease);
        Ok(lease)
    }

    /// Look up a lease by id.
    #[must_use]
    pub fn get(&self, id: i64) -> Option<&Lease> {
        self.leases.get(&id)
    }

    /// Number of live leases in the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.leases.len()
    }

    /// Whether the table holds no leases.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.leases.is_empty()
    }

    /// Explicitly revoke a lease, removing it from the table. Returns the
    /// removed lease, if it existed. Key deletion is the caller's job via
    /// [`Self::expire`] / [`revoke_keys`].
    pub fn revoke(&mut self, id: i64) -> Option<Lease> {
        self.leases.remove(&id)
    }

    /// The ids of every lease expired as of `now`, sorted ascending.
    #[must_use]
    pub fn expired(&self, now: UnixSeconds) -> Vec<i64> {
        self.leases
            .values()
            .filter(|l| l.is_expired(now))
            .map(|l| l.id)
            .collect()
    }

    /// Expire every lease whose TTL has elapsed by `now`: remove the lease from
    /// the table **and** delete all keys attached to it from `store`. Returns
    /// the list of `(lease_id, keys_deleted)`.
    ///
    /// This is the lease-expiry decision etcd applies on the lease lessor's
    /// tick. Deletion goes through [`Store::delete`], so each removed key gets a
    /// proper tombstone row + revision and shows up in watches.
    pub fn expire(&mut self, store: &mut Store, now: UnixSeconds) -> Vec<(i64, usize)> {
        let expired = self.expired(now);
        let mut report = Vec::with_capacity(expired.len());
        for id in expired {
            self.leases.remove(&id);
            let deleted = revoke_keys(store, id);
            report.push((id, deleted));
        }
        report
    }
}

/// Delete every live key attached to lease `id` from `store`, returning how
/// many keys were deleted. A no-op for lease `0` (no-lease) — those keys are
/// never lease-owned.
pub fn revoke_keys(store: &mut Store, id: i64) -> usize {
    if id == 0 {
        return 0;
    }
    let keys: Vec<Vec<u8>> = store
        .live_keys()
        .into_iter()
        .filter(|k| store.get_live(k).is_some_and(|r| r.lease == id))
        .collect();
    let mut deleted = 0;
    for k in keys {
        if store.delete(&k).ok().flatten().is_some() {
            deleted += 1;
        }
    }
    deleted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::range::{execute, RangeRequest};

    #[test]
    fn grant_rejects_zero_lease_id() {
        let mut t = LeaseTable::new();
        assert_eq!(t.grant(0, 10, 0), Err(KineError::InvalidLeaseId));
    }

    #[test]
    fn grant_rejects_non_positive_ttl() {
        let mut t = LeaseTable::new();
        assert_eq!(t.grant(1, 0, 0), Err(KineError::InvalidTtl { ttl_seconds: 0 }));
        assert_eq!(t.grant(1, -5, 0), Err(KineError::InvalidTtl { ttl_seconds: -5 }));
    }

    #[test]
    fn lease_expires_at_granted_plus_ttl_inclusive() {
        let l = Lease { id: 1, ttl_seconds: 10, granted_at: 100 };
        assert!(!l.is_expired(109));
        assert!(l.is_expired(110), "expiry boundary is inclusive");
        assert!(l.is_expired(200));
    }

    #[test]
    fn expired_lists_only_elapsed_leases() {
        let mut t = LeaseTable::new();
        t.grant(1, 10, 0).unwrap(); // expires at 10
        t.grant(2, 100, 0).unwrap(); // expires at 100
        assert_eq!(t.expired(10), vec![1]);
        assert_eq!(t.expired(100), vec![1, 2]);
        assert_eq!(t.expired(5), Vec::<i64>::new());
    }

    #[test]
    fn renewing_a_lease_extends_its_expiry() {
        let mut t = LeaseTable::new();
        t.grant(1, 10, 0).unwrap(); // expires at 10
        t.grant(1, 10, 8).unwrap(); // renewed: now expires at 18
        assert!(!t.expired(10).contains(&1));
        assert!(t.expired(18).contains(&1));
    }

    #[test]
    fn expire_deletes_keys_attached_to_the_lease() {
        let mut store = Store::new();
        store.create(b"/a", b"1", 7).unwrap(); // lease 7
        store.create(b"/b", b"2", 7).unwrap(); // lease 7
        store.create(b"/c", b"3", 0).unwrap(); // no lease
        let mut t = LeaseTable::new();
        t.grant(7, 10, 0).unwrap();

        let report = t.expire(&mut store, 10);
        assert_eq!(report, vec![(7, 2)], "both lease-7 keys deleted");
        // lease-owned keys gone, unleased key survives
        assert!(execute(&store, &RangeRequest::key(b"/a")).unwrap().kvs.is_empty());
        assert!(execute(&store, &RangeRequest::key(b"/b")).unwrap().kvs.is_empty());
        assert_eq!(execute(&store, &RangeRequest::key(b"/c")).unwrap().kvs[0].value, b"3");
        // lease removed from the table
        assert!(t.get(7).is_none());
    }

    #[test]
    fn expire_leaves_unexpired_leases_and_their_keys_alone() {
        let mut store = Store::new();
        store.create(b"/a", b"1", 7).unwrap();
        let mut t = LeaseTable::new();
        t.grant(7, 100, 0).unwrap();
        let report = t.expire(&mut store, 5);
        assert!(report.is_empty());
        assert_eq!(execute(&store, &RangeRequest::key(b"/a")).unwrap().kvs[0].value, b"1");
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn expiry_deletion_bumps_revision_and_is_watchable() {
        use crate::watch::{watch, EventKind};
        let mut store = Store::new();
        store.create(b"/a", b"1", 7).unwrap(); // rev 1
        let mut t = LeaseTable::new();
        t.grant(7, 10, 0).unwrap();
        t.expire(&mut store, 10); // rev 2 tombstone
        assert_eq!(store.current_revision(), 2);
        let evs = watch(&store, &RangeRequest::key(b"/a"), 1).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].kind, EventKind::Delete);
    }

    #[test]
    fn revoke_removes_lease_from_table() {
        let mut t = LeaseTable::new();
        t.grant(3, 10, 0).unwrap();
        assert!(t.revoke(3).is_some());
        assert!(t.is_empty());
        assert!(t.revoke(3).is_none());
    }

    #[test]
    fn revoke_keys_ignores_no_lease_sentinel() {
        let mut store = Store::new();
        store.create(b"/a", b"1", 0).unwrap();
        assert_eq!(revoke_keys(&mut store, 0), 0);
        assert_eq!(execute(&store, &RangeRequest::key(b"/a")).unwrap().kvs.len(), 1);
    }
}
