// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The monotonic global revision — kine's translation of etcd's MVCC clock.
//!
//! In etcd every mutation across the whole keyspace bumps one cluster-wide
//! 64-bit revision. kine reproduces this exactly: its backing SQL table has a
//! monotonically increasing `id` (the auto-increment primary key) that plays
//! the role of the global revision, and each visible row records the revision
//! at which the key was first created (`create_revision`) and last modified
//! (`mod_revision`). This module owns that counter as pure logic; the SQL
//! sequence that realises it in a real driver is modelled (not executed) in
//! [`crate::sql`].
//!
//! Reference: etcd MVCC design (`etcd-io/etcd`, `mvcc/revision.go`) and the
//! kine generic backend (`k3s-io/kine`, `pkg/server`), both Apache-2.0.

use crate::error::{KineError, Result};

/// A global revision. Mirrors etcd's `int64` main revision: monotonic,
/// non-negative, and `0` is the sentinel meaning "the latest / current
/// revision" in a read request.
pub type Revision = i64;

/// The reserved request-side value meaning "read at the current revision".
pub const CURRENT: Revision = 0;

/// The monotonic counter that hands out a fresh revision per mutation.
///
/// Each successful create / update / delete calls [`Clock::next`] exactly once,
/// reproducing etcd's "one revision per write transaction" rule. The counter
/// never goes backwards and never repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    current: Revision,
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock {
    /// A fresh clock. etcd's empty store starts at revision `1` after the first
    /// write; before any write the current revision is `0` (nothing exists).
    #[must_use]
    pub const fn new() -> Self {
        Self { current: 0 }
    }

    /// The most recently issued revision (the store's "header revision"). `0`
    /// before any mutation has occurred.
    #[must_use]
    pub const fn current(&self) -> Revision {
        self.current
    }

    /// Issue the next revision and advance the clock. Returns the new current
    /// revision. Saturates at [`i64::MAX`] rather than wrapping — a store that
    /// reaches 2^63 writes is far past any real lifetime, and saturation keeps
    /// the monotonicity invariant instead of silently aliasing old revisions.
    ///
    /// Named `next` for the "next revision" domain meaning; this is a counter
    /// bump, not an iterator step.
    #[allow(clippy::should_implement_trait)]
    pub const fn next(&mut self) -> Revision {
        self.current = self.current.saturating_add(1);
        self.current
    }

    /// Resolve a request-side revision into the concrete revision to read at.
    ///
    /// * `CURRENT` (`0`) resolves to the store's current revision.
    /// * A positive revision is returned as-is.
    ///
    /// # Errors
    /// * [`KineError::NegativeRevision`] if `requested` is negative.
    /// * [`KineError::FutureRevision`] if `requested` exceeds the current
    ///   revision — etcd's `ErrFutureRev`.
    pub const fn resolve_read(&self, requested: Revision) -> Result<Revision> {
        if requested < 0 {
            return Err(KineError::NegativeRevision { revision: requested });
        }
        if requested == CURRENT {
            return Ok(self.current);
        }
        if requested > self.current {
            return Err(KineError::FutureRevision {
                requested,
                current: self.current,
            });
        }
        Ok(requested)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_clock_starts_at_zero() {
        assert_eq!(Clock::new().current(), 0);
    }

    #[test]
    fn next_is_strictly_monotonic() {
        let mut c = Clock::new();
        let a = c.next();
        let b = c.next();
        let d = c.next();
        assert_eq!((a, b, d), (1, 2, 3));
        assert!(a < b && b < d);
    }

    #[test]
    fn resolve_current_returns_head() {
        let mut c = Clock::new();
        c.next();
        c.next();
        assert_eq!(c.resolve_read(CURRENT).unwrap(), 2);
    }

    #[test]
    fn resolve_past_revision_is_passed_through() {
        let mut c = Clock::new();
        for _ in 0..5 {
            c.next();
        }
        assert_eq!(c.resolve_read(3).unwrap(), 3);
    }

    #[test]
    fn resolve_future_revision_is_rejected() {
        let mut c = Clock::new();
        c.next();
        assert_eq!(
            c.resolve_read(9),
            Err(KineError::FutureRevision { requested: 9, current: 1 })
        );
    }

    #[test]
    fn resolve_negative_revision_is_rejected() {
        let c = Clock::new();
        assert_eq!(
            c.resolve_read(-4),
            Err(KineError::NegativeRevision { revision: -4 })
        );
    }
}
