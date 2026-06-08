// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Error type for the kine decision core.
//!
//! Every fallible path returns a `Result`; the core never panics on bad input
//! (Charter forbids `panic!` / `unwrap` / `expect` in shipped code). All
//! variants are recoverable — a caller (the apiserver, in production) maps them
//! onto the corresponding etcd gRPC status codes.
//!
//! These errors are INFRASTRUCTURE-internal (ADR-004 §6.3): kine is hidden
//! from the homeowner, so the messages model etcd / kine wire semantics, not
//! grandma-friendly prose.

/// Why a kine store operation failed.
///
/// The string forms deliberately echo the etcd MVCC error vocabulary so that a
/// future gRPC layer (Phase-1b) can translate them faithfully — most notably
/// [`Self::Compacted`], which etcd surfaces as the well-known
/// `"mvcc: required revision has been compacted"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KineError {
    /// A read asked for a `revision` older than the store's compacted floor.
    /// etcd reports this as `ErrCompacted` — `"mvcc: required revision has
    /// been compacted"`. Carries the requested revision and the floor below
    /// which history no longer exists.
    Compacted { requested: i64, compacted: i64 },
    /// A read asked for a `revision` that the store has not yet reached. etcd
    /// reports this as `ErrFutureRev`.
    FutureRevision { requested: i64, current: i64 },
    /// A revision argument was negative. Revisions are monotonic and
    /// non-negative (`0` means "current"); a negative value is malformed.
    NegativeRevision { revision: i64 },
    /// A `compact` was asked to move the compacted floor backwards (to a
    /// revision at or below the current floor). etcd rejects this as
    /// `ErrCompacted` too, but we keep it distinct for clarity.
    CompactionNotForward { requested: i64, current: i64 },
    /// A `compact` targeted a revision the store has not yet reached.
    CompactFutureRevision { requested: i64, current: i64 },
    /// A Range request supplied an empty key (etcd forbids an empty key for a
    /// point get; an empty key only has meaning as a range bound).
    EmptyKey,
    /// A Range request supplied a `range_end` that sorts at or before `key`,
    /// which can never select anything. We reject rather than silently return
    /// an empty result, mirroring etcd's `ErrEmptyKey` / invalid-range guard.
    InvalidRange,
    /// A `limit` was negative.
    NegativeLimit { limit: i64 },
    /// A lease id of `0` was used to attach a key. etcd reserves lease id `0`
    /// for "no lease"; attaching to it is a client error.
    InvalidLeaseId,
    /// A non-positive TTL was supplied when granting a lease.
    InvalidTtl { ttl_seconds: i64 },
}

impl core::fmt::Display for KineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Compacted { requested, compacted } => write!(
                f,
                "mvcc: required revision has been compacted (requested {requested}, compacted {compacted})"
            ),
            Self::FutureRevision { requested, current } => write!(
                f,
                "mvcc: required revision is a future revision (requested {requested}, current {current})"
            ),
            Self::NegativeRevision { revision } => {
                write!(f, "revision {revision} is negative")
            }
            Self::CompactionNotForward { requested, current } => write!(
                f,
                "compact revision {requested} must be greater than the current compacted revision {current}"
            ),
            Self::CompactFutureRevision { requested, current } => write!(
                f,
                "compact revision {requested} is a future revision (current {current})"
            ),
            Self::EmptyKey => f.write_str("etcdserver: key is not provided"),
            Self::InvalidRange => {
                f.write_str("etcdserver: range_end must sort after key")
            }
            Self::NegativeLimit { limit } => write!(f, "limit {limit} is negative"),
            Self::InvalidLeaseId => {
                f.write_str("etcdserver: lease id 0 is reserved for no-lease")
            }
            Self::InvalidTtl { ttl_seconds } => {
                write!(f, "lease TTL {ttl_seconds}s must be positive")
            }
        }
    }
}

impl std::error::Error for KineError {}

/// Convenience alias for the crate's fallible results.
pub type Result<T> = core::result::Result<T, KineError>;
