// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The etcd `Txn` RPC — guarded compare-and-swap, the core of every Kubernetes
//! optimistic write.
//!
//! A transaction carries three lists:
//!
//! * `compares` — predicates over individual keys. The txn **succeeds** iff
//!   *every* compare holds (an empty list always succeeds), exactly etcd's
//!   "all comparisons are logically AND'd" rule.
//! * `success` — the ops to run when the txn succeeds.
//! * `failure` — the ops to run when it does not.
//!
//! Each compare tests one *target* of a key — its `create_revision`, `version`,
//! `mod_revision`, `value`, or `lease` — against an operand with one of four
//! operators (`Equal` / `NotEqual` / `Greater` / `Less`). A **missing** key
//! reads as the etcd zero tuple (`create_revision = mod_revision = version =
//! lease = 0`, empty value), which is what makes `create_revision == 0` the
//! canonical "create if absent" guard the apiserver issues.
//!
//! # Scope (honest port note)
//!
//! This models the etcd Txn **selection** semantics — the compare guard and
//! branch choice, which is the subtle, well-specified part — and executes the
//! chosen ops through the existing [`Store`] primitives. Writes therefore take
//! **sequential revisions**, one per row, exactly as kine assigns them (kine's
//! revision *is* the SQL row's auto-increment id; one INSERT, one revision).
//! The single-write compare-and-swap — the *only* txn shape the apiserver and
//! kine actually use — is thus etcd-faithful end to end. etcd's stronger
//! single-revision-per-*multi*-write-txn guarantee is neither exercised by kine
//! nor claimed here; see the parity manifest.
//!
//! Reference: etcd `etcdserverpb.TxnRequest` / `Compare` semantics and
//! `clientv3` `Txn().If().Then().Else()`; kine `pkg/server` `Txn`. Behavioural
//! reimplementation from public sources, Apache-2.0.

use crate::error::{KineError, Result};
use crate::range::{execute, RangeEnd, RangeRequest, RangeResponse};
use crate::revision::Revision;
use crate::store::Store;

/// Which property of a key a [`Compare`] tests. Mirrors etcd's
/// `Compare.target`. Four of the five are numeric; only [`Self::Value`] is
/// byte-valued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareTarget {
    /// The key's `version` — the number of writes in its current generation,
    /// `1` on create, incremented per update, `0` for a missing key.
    Version,
    /// The key's `create_revision`; `0` for a missing key.
    CreateRevision,
    /// The key's `mod_revision`; `0` for a missing key.
    ModRevision,
    /// The key's stored bytes; empty for a missing key.
    Value,
    /// The lease id attached to the key; `0` for none / a missing key.
    Lease,
}

/// The comparison operator of a [`Compare`]. Mirrors etcd's `Compare.result`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareResult {
    /// The target equals the operand.
    Equal,
    /// The target differs from the operand.
    NotEqual,
    /// The target sorts strictly after the operand (numeric `>` or lexical for
    /// [`CompareTarget::Value`]).
    Greater,
    /// The target sorts strictly before the operand.
    Less,
}

/// The right-hand side of a [`Compare`]: an integer for the numeric targets, or
/// bytes for [`CompareTarget::Value`]. A mismatch with the target is rejected
/// as [`KineError::TxnCompareTypeMismatch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareOperand {
    /// An integer operand (for `Version` / `CreateRevision` / `ModRevision` /
    /// `Lease`).
    Int(i64),
    /// A byte operand (for `Value`).
    Bytes(Vec<u8>),
}

/// A single guard predicate over one key, evaluated against the live store.
///
/// A txn succeeds iff *every* compare holds. A predicate on a missing key sees
/// the etcd zero tuple — `version == create_revision == mod_revision == lease
/// == 0`, empty value — so `CreateRevision Equal 0` is precisely "the key does
/// not exist".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compare {
    /// The key whose property is being tested. Must be non-empty.
    pub key: Vec<u8>,
    /// Which property of the key to read.
    pub target: CompareTarget,
    /// The operator relating the target to the operand.
    pub result: CompareResult,
    /// The value to compare against.
    pub operand: CompareOperand,
}

impl Compare {
    /// Compare the key's `create_revision` against `revision`.
    #[must_use]
    pub fn create_revision(key: &[u8], result: CompareResult, revision: i64) -> Self {
        Self::numeric(key, CompareTarget::CreateRevision, result, revision)
    }

    /// Compare the key's `mod_revision` against `revision` — the compare-and-
    /// swap guard the apiserver issues for every optimistic update.
    #[must_use]
    pub fn mod_revision(key: &[u8], result: CompareResult, revision: i64) -> Self {
        Self::numeric(key, CompareTarget::ModRevision, result, revision)
    }

    /// Compare the key's `version` against `version`.
    #[must_use]
    pub fn version(key: &[u8], result: CompareResult, version: i64) -> Self {
        Self::numeric(key, CompareTarget::Version, result, version)
    }

    /// Compare the key's attached lease id against `lease`.
    #[must_use]
    pub fn lease(key: &[u8], result: CompareResult, lease: i64) -> Self {
        Self::numeric(key, CompareTarget::Lease, result, lease)
    }

    /// Compare the key's stored value against `value` (lexical for the ordering
    /// operators).
    #[must_use]
    pub fn value(key: &[u8], result: CompareResult, value: &[u8]) -> Self {
        Self {
            key: key.to_vec(),
            target: CompareTarget::Value,
            result,
            operand: CompareOperand::Bytes(value.to_vec()),
        }
    }

    /// Shared builder for the four numeric-target compares.
    #[must_use]
    fn numeric(key: &[u8], target: CompareTarget, result: CompareResult, operand: i64) -> Self {
        Self { key: key.to_vec(), target, result, operand: CompareOperand::Int(operand) }
    }
}

/// One operation in a txn branch. `Range` reads; `Put` and `DeleteRange` write.
#[derive(Debug, Clone)]
pub enum TxnOp {
    /// A read — the etcd `Range` RPC, returning a [`RangeResponse`].
    Range(RangeRequest),
    /// An unconditional put of `value` at `key` with `lease` (`0` = none).
    Put {
        /// The key to write.
        key: Vec<u8>,
        /// The bytes to store.
        value: Vec<u8>,
        /// The lease to attach (`0` = none).
        lease: i64,
    },
    /// Delete every live key the `[key, end)` selector matches.
    DeleteRange {
        /// The lower-bound / point key.
        key: Vec<u8>,
        /// The upper-bound selector.
        end: RangeEnd,
    },
}

impl TxnOp {
    /// An unleased put of `value` at `key`.
    #[must_use]
    pub fn put(key: &[u8], value: &[u8]) -> Self {
        Self::Put { key: key.to_vec(), value: value.to_vec(), lease: 0 }
    }

    /// A put of `value` at `key` attached to `lease`.
    #[must_use]
    pub fn put_leased(key: &[u8], value: &[u8], lease: i64) -> Self {
        Self::Put { key: key.to_vec(), value: value.to_vec(), lease }
    }

    /// A point read of `key`.
    #[must_use]
    pub fn get(key: &[u8]) -> Self {
        Self::Range(RangeRequest::key(key))
    }

    /// An arbitrary range read.
    #[must_use]
    pub const fn range(req: RangeRequest) -> Self {
        Self::Range(req)
    }

    /// A point delete of `key`.
    #[must_use]
    pub fn delete(key: &[u8]) -> Self {
        Self::DeleteRange { key: key.to_vec(), end: RangeEnd::Single }
    }

    /// A prefix delete of everything under `prefix`.
    #[must_use]
    pub fn delete_prefix(prefix: &[u8]) -> Self {
        Self::DeleteRange { key: prefix.to_vec(), end: RangeEnd::Prefix }
    }
}

/// A guarded transaction: run `success` if every compare holds, else `failure`.
#[derive(Debug, Clone, Default)]
pub struct Txn {
    /// The guard predicates, logically AND'd.
    pub compares: Vec<Compare>,
    /// Ops to run when the guard holds.
    pub success: Vec<TxnOp>,
    /// Ops to run when it does not.
    pub failure: Vec<TxnOp>,
}

impl Txn {
    /// An empty txn (no compares ⇒ always succeeds; no ops).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a txn with the given guard predicates.
    #[must_use]
    pub const fn when(compares: Vec<Compare>) -> Self {
        Self { compares, success: Vec::new(), failure: Vec::new() }
    }

    /// Set the success (`then`) ops.
    #[must_use]
    pub fn and_then(mut self, ops: Vec<TxnOp>) -> Self {
        self.success = ops;
        self
    }

    /// Set the failure (`else`) ops.
    #[must_use]
    pub fn or_else(mut self, ops: Vec<TxnOp>) -> Self {
        self.failure = ops;
        self
    }
}

/// The result of one executed [`TxnOp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxnOpResponse {
    /// The result of a `Range` read.
    Range(RangeResponse),
    /// The result of a `Put`: the revision the write took.
    Put {
        /// Revision assigned to the written row.
        revision: Revision,
    },
    /// The result of a `DeleteRange`: how many live keys were deleted and the
    /// store revision after the deletes.
    DeleteRange {
        /// Number of live keys deleted.
        deleted: i64,
        /// Store revision after the deletes (unchanged if nothing matched).
        revision: Revision,
    },
}

/// The result of an applied [`Txn`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxnResponse {
    /// Whether the guard held and the `success` branch ran.
    pub succeeded: bool,
    /// The store's revision after the txn (its header revision).
    pub revision: Revision,
    /// One response per executed op, in branch order.
    pub responses: Vec<TxnOpResponse>,
}

/// Apply a [`Txn`] to the store: validate, evaluate the guard, run the chosen
/// branch, and report each op's result.
///
/// The whole request is validated *before* any write, so a malformed txn never
/// half-applies. The single-write compare-and-swap — the only shape kine and
/// the apiserver use — is etcd-faithful; a multi-write branch takes sequential
/// revisions (kine's one-row-one-revision model, see the module header).
///
/// # Errors
/// * [`KineError::EmptyKey`] — a compare or write op carried an empty key.
/// * [`KineError::TxnCompareTypeMismatch`] — a compare operand's kind did not
///   match its target.
/// * [`KineError::TxnDuplicateKey`] — a branch wrote one key more than once.
/// * Any error surfaced by an executed `Range` op (e.g.
///   [`KineError::Compacted`]).
pub fn apply(store: &mut Store, txn: &Txn) -> Result<TxnResponse> {
    // Validate the entire request up front — both branches — so nothing is
    // applied if any part is malformed.
    for c in &txn.compares {
        validate_compare(c)?;
    }
    check_disjoint_writes(&txn.success)?;
    check_disjoint_writes(&txn.failure)?;

    let succeeded = txn.compares.iter().try_fold(true, |acc, c| {
        Ok::<bool, KineError>(acc && eval_compare(store, c)?)
    })?;

    let ops = if succeeded { &txn.success } else { &txn.failure };
    let responses = run_ops(store, ops)?;

    Ok(TxnResponse { succeeded, revision: store.current_revision(), responses })
}

/// Reject a compare whose operand kind does not match its target, or whose key
/// is empty.
fn validate_compare(c: &Compare) -> Result<()> {
    if c.key.is_empty() {
        return Err(KineError::EmptyKey);
    }
    let ok = match c.target {
        CompareTarget::Value => matches!(c.operand, CompareOperand::Bytes(_)),
        CompareTarget::Version
        | CompareTarget::CreateRevision
        | CompareTarget::ModRevision
        | CompareTarget::Lease => matches!(c.operand, CompareOperand::Int(_)),
    };
    if ok {
        Ok(())
    } else {
        Err(KineError::TxnCompareTypeMismatch)
    }
}

/// Evaluate one compare against the live store. A missing key reads as the etcd
/// zero tuple. Assumes [`validate_compare`] has already passed.
fn eval_compare(store: &Store, c: &Compare) -> Result<bool> {
    let live = store.get_live(&c.key);
    let ordering = match (c.target, &c.operand) {
        (CompareTarget::Value, CompareOperand::Bytes(want)) => {
            let have: &[u8] = live.map_or(&[], |r| &r.value);
            have.cmp(want.as_slice())
        }
        (target, CompareOperand::Int(want)) => {
            let have = match target {
                CompareTarget::CreateRevision => live.map_or(0, |r| r.create_revision),
                CompareTarget::ModRevision => live.map_or(0, |r| r.mod_revision),
                CompareTarget::Lease => live.map_or(0, |r| r.lease),
                CompareTarget::Version => version_of(store, &c.key),
                CompareTarget::Value => unreachable!("validated as Int target"),
            };
            have.cmp(want)
        }
        // Excluded by validate_compare.
        _ => return Err(KineError::TxnCompareTypeMismatch),
    };
    Ok(match c.result {
        CompareResult::Equal => ordering.is_eq(),
        CompareResult::NotEqual => ordering.is_ne(),
        CompareResult::Greater => ordering.is_gt(),
        CompareResult::Less => ordering.is_lt(),
    })
}

/// The etcd `version` of `key`: writes in the current live generation, or `0`
/// if the key has no live row.
fn version_of(store: &Store, key: &[u8]) -> i64 {
    let Some(generation) = store.get_live(key).map(|r| r.create_revision) else {
        return 0;
    };
    store
        .rows()
        .iter()
        .filter(|r| r.key == key && r.create_revision == generation && !r.deleted)
        .count() as i64
}

/// Run a branch's ops in order, after confirming its writes touch disjoint keys.
fn run_ops(store: &mut Store, ops: &[TxnOp]) -> Result<Vec<TxnOpResponse>> {
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        let resp = match op {
            TxnOp::Range(req) => TxnOpResponse::Range(execute(store, req)?),
            TxnOp::Put { key, value, lease } => {
                let revision = store.put(key, value, *lease)?;
                TxnOpResponse::Put { revision }
            }
            TxnOp::DeleteRange { key, end } => {
                let deleted = delete_range(store, key, end)?;
                TxnOpResponse::DeleteRange { deleted, revision: store.current_revision() }
            }
        };
        out.push(resp);
    }
    Ok(out)
}

/// Delete every live key matched by the `[key, end)` selector, returning the
/// count. Reuses the [`execute`] range plan to pick the keys, then tombstones
/// each (one revision per delete — kine's row-per-write model).
fn delete_range(store: &mut Store, key: &[u8], end: &RangeEnd) -> Result<i64> {
    let req = RangeRequest { key: key.to_vec(), end: end.clone(), revision: 0, limit: 0 };
    let victims: Vec<Vec<u8>> = execute(store, &req)?.kvs.into_iter().map(|r| r.key).collect();
    let mut deleted = 0;
    for k in victims {
        if store.delete(&k)?.is_some() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Reject a branch whose write ops (`Put` / `DeleteRange`) have overlapping key
/// spans — etcd's "duplicate key given in txn request" guard. Read ops are
/// ignored. O(n²) over a branch's ops, which is tiny in practice.
fn check_disjoint_writes(ops: &[TxnOp]) -> Result<()> {
    let spans: Vec<Span> = ops.iter().filter_map(write_span).collect();
    for (i, a) in spans.iter().enumerate() {
        for b in &spans[i + 1..] {
            if a.overlaps(b) {
                return Err(KineError::TxnDuplicateKey);
            }
        }
    }
    Ok(())
}

/// A half-open key span `[lo, hi)`; `hi == None` means unbounded (to the end of
/// the keyspace).
struct Span {
    lo: Vec<u8>,
    hi: Option<Vec<u8>>,
}

impl Span {
    /// Do these two half-open spans share any key?
    fn overlaps(&self, other: &Self) -> bool {
        let a_lo_before_b_hi = other.hi.as_ref().is_none_or(|h| self.lo < *h);
        let b_lo_before_a_hi = self.hi.as_ref().is_none_or(|h| other.lo < *h);
        a_lo_before_b_hi && b_lo_before_a_hi
    }
}

/// The key span a write op affects, or `None` for a read op.
fn write_span(op: &TxnOp) -> Option<Span> {
    match op {
        TxnOp::Range(_) => None,
        TxnOp::Put { key, .. } => {
            let mut hi = key.clone();
            hi.push(0); // the immediate successor of the point key
            Some(Span { lo: key.clone(), hi: Some(hi) })
        }
        TxnOp::DeleteRange { key, end } => Some(Span { lo: key.clone(), hi: span_upper(end, key) }),
    }
}

/// The exclusive upper bound of a `[key, end)` selector; `None` if unbounded.
fn span_upper(end: &RangeEnd, key: &[u8]) -> Option<Vec<u8>> {
    match end {
        RangeEnd::Single => {
            let mut hi = key.to_vec();
            hi.push(0);
            Some(hi)
        }
        RangeEnd::Prefix => prefix_upper(key),
        RangeEnd::Explicit(e) => Some(e.clone()),
        RangeEnd::AllKeys => None,
    }
}

/// The exclusive upper bound of a prefix scan, or `None` when the prefix is
/// empty or all-`0xFF` (i.e. unbounded to the end of the keyspace).
fn prefix_upper(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.last_mut() {
        if *last < 0xFF {
            *last += 1;
            return Some(end);
        }
        end.pop();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn seeded() -> Store {
        let mut s = Store::new();
        s.create(b"/k/a", b"va", 0).unwrap(); // rev 1
        s.create(b"/k/b", b"vb", 0).unwrap(); // rev 2
        s
    }

    #[test]
    fn empty_compare_list_always_succeeds() {
        let mut s = Store::new();
        let txn = Txn::when(vec![]).and_then(vec![TxnOp::put(b"/k/x", b"1")]);
        let resp = apply(&mut s, &txn).unwrap();
        assert!(resp.succeeded);
        assert_eq!(s.get_live(b"/k/x").unwrap().value, b"1");
    }

    #[test]
    fn mod_revision_guard_runs_success_when_matched() {
        let mut s = seeded(); // /k/a at mod_revision 1
        let txn = Txn::when(vec![Compare::mod_revision(b"/k/a", CompareResult::Equal, 1)])
            .and_then(vec![TxnOp::put(b"/k/a", b"va2")])
            .or_else(vec![TxnOp::get(b"/k/a")]);
        let resp = apply(&mut s, &txn).unwrap();
        assert!(resp.succeeded);
        assert_eq!(s.get_live(b"/k/a").unwrap().value, b"va2");
        // the put took the next revision (3) and is reported.
        match &resp.responses[0] {
            TxnOpResponse::Put { revision } => assert_eq!(*revision, 3),
            other => panic!("expected Put response, got {other:?}"),
        }
    }

    #[test]
    fn mod_revision_guard_runs_failure_when_stale() {
        let mut s = seeded();
        // Guard on a stale revision: success put must NOT run, failure get does.
        let txn = Txn::when(vec![Compare::mod_revision(b"/k/a", CompareResult::Equal, 99)])
            .and_then(vec![TxnOp::put(b"/k/a", b"clobber")])
            .or_else(vec![TxnOp::get(b"/k/a")]);
        let resp = apply(&mut s, &txn).unwrap();
        assert!(!resp.succeeded);
        assert_eq!(s.get_live(b"/k/a").unwrap().value, b"va", "value untouched");
        assert_eq!(s.current_revision(), 2, "no write happened");
        match &resp.responses[0] {
            TxnOpResponse::Range(r) => assert_eq!(r.kvs[0].value, b"va"),
            other => panic!("expected Range response, got {other:?}"),
        }
    }

    #[test]
    fn create_revision_zero_is_the_create_if_absent_idiom() {
        let mut s = Store::new();
        // "create if absent": create_revision == 0 holds only for a missing key.
        let txn = Txn::when(vec![Compare::create_revision(b"/k/new", CompareResult::Equal, 0)])
            .and_then(vec![TxnOp::put(b"/k/new", b"first")]);
        let resp = apply(&mut s, &txn).unwrap();
        assert!(resp.succeeded);
        assert_eq!(s.get_live(b"/k/new").unwrap().value, b"first");
    }

    #[test]
    fn create_if_absent_fails_when_key_exists() {
        let mut s = seeded();
        let txn = Txn::when(vec![Compare::create_revision(b"/k/a", CompareResult::Equal, 0)])
            .and_then(vec![TxnOp::put(b"/k/a", b"clobber")])
            .or_else(vec![TxnOp::get(b"/k/a")]);
        let resp = apply(&mut s, &txn).unwrap();
        assert!(!resp.succeeded, "key exists, so create_revision != 0");
        assert_eq!(s.get_live(b"/k/a").unwrap().value, b"va");
    }

    #[test]
    fn version_compare_counts_writes_in_the_generation() {
        let mut s = Store::new();
        s.create(b"/k/a", b"1", 0).unwrap(); // version 1
        s.update(b"/k/a", b"2", 0).unwrap(); // version 2
        let hit = Txn::when(vec![Compare::version(b"/k/a", CompareResult::Equal, 2)]);
        assert!(apply(&mut s.clone(), &hit).unwrap().succeeded);
        let miss = Txn::when(vec![Compare::version(b"/k/a", CompareResult::Equal, 1)]);
        assert!(!apply(&mut s, &miss).unwrap().succeeded);
    }

    #[test]
    fn greater_and_less_operators_on_mod_revision() {
        let mut s = seeded(); // /k/a mod_revision 1
        let gt = Txn::when(vec![Compare::mod_revision(b"/k/a", CompareResult::Greater, 0)]);
        assert!(apply(&mut s.clone(), &gt).unwrap().succeeded);
        let lt = Txn::when(vec![Compare::mod_revision(b"/k/a", CompareResult::Less, 5)]);
        assert!(apply(&mut s.clone(), &lt).unwrap().succeeded);
        let not_lt = Txn::when(vec![Compare::mod_revision(b"/k/a", CompareResult::Less, 1)]);
        assert!(!apply(&mut s, &not_lt).unwrap().succeeded);
    }

    #[test]
    fn value_equal_and_not_equal_compare() {
        let mut s = seeded();
        let eq = Txn::when(vec![Compare::value(b"/k/a", CompareResult::Equal, b"va")]);
        assert!(apply(&mut s.clone(), &eq).unwrap().succeeded);
        let ne = Txn::when(vec![Compare::value(b"/k/a", CompareResult::NotEqual, b"zz")]);
        assert!(apply(&mut s.clone(), &ne).unwrap().succeeded);
        let wrong = Txn::when(vec![Compare::value(b"/k/a", CompareResult::Equal, b"zz")]);
        assert!(!apply(&mut s, &wrong).unwrap().succeeded);
    }

    #[test]
    fn lease_compare_reads_the_attached_lease() {
        let mut s = Store::new();
        s.create(b"/k/a", b"v", 42).unwrap();
        let hit = Txn::when(vec![Compare::lease(b"/k/a", CompareResult::Equal, 42)]);
        assert!(apply(&mut s.clone(), &hit).unwrap().succeeded);
        let miss = Txn::when(vec![Compare::lease(b"/k/a", CompareResult::Equal, 0)]);
        assert!(!apply(&mut s, &miss).unwrap().succeeded);
    }

    #[test]
    fn compares_are_anded_together() {
        let mut s = seeded();
        let both_true = Txn::when(vec![
            Compare::mod_revision(b"/k/a", CompareResult::Equal, 1),
            Compare::value(b"/k/b", CompareResult::Equal, b"vb"),
        ]);
        assert!(apply(&mut s.clone(), &both_true).unwrap().succeeded);
        let one_false = Txn::when(vec![
            Compare::mod_revision(b"/k/a", CompareResult::Equal, 1),
            Compare::value(b"/k/b", CompareResult::Equal, b"WRONG"),
        ]);
        assert!(!apply(&mut s, &one_false).unwrap().succeeded);
    }

    #[test]
    fn success_branch_range_op_returns_a_range_response() {
        let mut s = seeded();
        let txn = Txn::when(vec![]).and_then(vec![TxnOp::range(
            crate::RangeRequest::prefix(b"/k/"),
        )]);
        let resp = apply(&mut s, &txn).unwrap();
        match &resp.responses[0] {
            TxnOpResponse::Range(r) => assert_eq!(r.count, 2),
            other => panic!("expected Range, got {other:?}"),
        }
    }

    #[test]
    fn delete_op_removes_key_and_reports_count() {
        let mut s = seeded();
        let txn = Txn::when(vec![]).and_then(vec![TxnOp::delete(b"/k/a")]);
        let resp = apply(&mut s, &txn).unwrap();
        assert!(s.get_live(b"/k/a").is_none());
        match &resp.responses[0] {
            TxnOpResponse::DeleteRange { deleted, .. } => assert_eq!(*deleted, 1),
            other => panic!("expected DeleteRange, got {other:?}"),
        }
    }

    #[test]
    fn delete_prefix_removes_the_subtree_and_counts_all() {
        let mut s = seeded();
        s.create(b"/other/z", b"z", 0).unwrap();
        let txn = Txn::when(vec![]).and_then(vec![TxnOp::delete_prefix(b"/k/")]);
        let resp = apply(&mut s, &txn).unwrap();
        assert!(s.get_live(b"/k/a").is_none());
        assert!(s.get_live(b"/k/b").is_none());
        assert!(s.get_live(b"/other/z").is_some(), "sibling subtree untouched");
        match &resp.responses[0] {
            TxnOpResponse::DeleteRange { deleted, .. } => assert_eq!(*deleted, 2),
            other => panic!("expected DeleteRange, got {other:?}"),
        }
    }

    #[test]
    fn deleting_an_absent_key_reports_zero() {
        let mut s = Store::new();
        let txn = Txn::when(vec![]).and_then(vec![TxnOp::delete(b"/k/ghost")]);
        let resp = apply(&mut s, &txn).unwrap();
        match &resp.responses[0] {
            TxnOpResponse::DeleteRange { deleted, .. } => assert_eq!(*deleted, 0),
            other => panic!("expected DeleteRange, got {other:?}"),
        }
        assert_eq!(s.current_revision(), 0, "no-op delete does not bump revision");
    }

    #[test]
    fn failure_branch_never_applies_success_writes() {
        let mut s = seeded();
        let txn = Txn::when(vec![Compare::version(b"/k/a", CompareResult::Equal, 99)])
            .and_then(vec![TxnOp::put(b"/k/a", b"x"), TxnOp::delete(b"/k/b")])
            .or_else(vec![]);
        let resp = apply(&mut s, &txn).unwrap();
        assert!(!resp.succeeded);
        assert!(resp.responses.is_empty(), "empty failure branch ran nothing");
        assert_eq!(s.get_live(b"/k/a").unwrap().value, b"va");
        assert!(s.get_live(b"/k/b").is_some());
        assert_eq!(s.current_revision(), 2, "no success write leaked");
    }

    #[test]
    fn response_header_revision_reflects_the_post_txn_store() {
        let mut s = seeded(); // revision 2
        let txn = Txn::when(vec![]).and_then(vec![TxnOp::put(b"/k/c", b"vc")]);
        let resp = apply(&mut s, &txn).unwrap();
        assert_eq!(resp.revision, 3);
        assert_eq!(s.current_revision(), 3);
    }

    #[test]
    fn compare_operand_type_mismatch_is_rejected() {
        let mut s = seeded();
        // A value target with an integer operand is malformed.
        let bad = Txn::when(vec![Compare {
            key: b"/k/a".to_vec(),
            target: CompareTarget::Value,
            result: CompareResult::Equal,
            operand: CompareOperand::Int(1),
        }]);
        assert_eq!(apply(&mut s, &bad), Err(KineError::TxnCompareTypeMismatch));
    }

    #[test]
    fn integer_target_with_bytes_operand_is_rejected() {
        let mut s = seeded();
        let bad = Txn::when(vec![Compare {
            key: b"/k/a".to_vec(),
            target: CompareTarget::ModRevision,
            result: CompareResult::Equal,
            operand: CompareOperand::Bytes(b"1".to_vec()),
        }]);
        assert_eq!(apply(&mut s, &bad), Err(KineError::TxnCompareTypeMismatch));
    }

    #[test]
    fn duplicate_key_across_writes_in_one_branch_is_rejected() {
        let mut s = Store::new();
        let txn = Txn::when(vec![])
            .and_then(vec![TxnOp::put(b"/k/a", b"1"), TxnOp::put(b"/k/a", b"2")]);
        assert_eq!(apply(&mut s, &txn), Err(KineError::TxnDuplicateKey));
    }

    #[test]
    fn put_and_delete_of_the_same_key_in_one_branch_is_rejected() {
        let mut s = seeded();
        let txn = Txn::when(vec![])
            .and_then(vec![TxnOp::put(b"/k/a", b"1"), TxnOp::delete(b"/k/a")]);
        assert_eq!(apply(&mut s, &txn), Err(KineError::TxnDuplicateKey));
    }

    #[test]
    fn empty_compare_key_is_rejected() {
        let mut s = seeded();
        let bad = Txn::when(vec![Compare::mod_revision(b"", CompareResult::Equal, 1)]);
        assert_eq!(apply(&mut s, &bad), Err(KineError::EmptyKey));
    }

    #[test]
    fn a_read_only_txn_does_not_advance_the_revision() {
        let mut s = seeded();
        let txn = Txn::when(vec![Compare::mod_revision(b"/k/a", CompareResult::Equal, 1)])
            .and_then(vec![TxnOp::get(b"/k/a"), TxnOp::range(RangeRequest::prefix(b"/k/"))]);
        let resp = apply(&mut s, &txn).unwrap();
        assert!(resp.succeeded);
        assert_eq!(s.current_revision(), 2, "pure reads keep the revision");
        assert_eq!(resp.responses.len(), 2);
    }

    // A multi-write success branch takes SEQUENTIAL revisions — kine's
    // one-row-one-revision model (NOT etcd's single-revision-per-txn, which
    // kine never exercises). Documented in the module header + parity manifest.
    #[test]
    fn multi_write_branch_takes_sequential_revisions_kine_model() {
        let mut s = Store::new(); // revision 0
        let txn = Txn::when(vec![])
            .and_then(vec![TxnOp::put(b"/k/a", b"1"), TxnOp::put(b"/k/b", b"2")]);
        let resp = apply(&mut s, &txn).unwrap();
        let revs: Vec<_> = resp
            .responses
            .iter()
            .map(|r| match r {
                TxnOpResponse::Put { revision } => *revision,
                _ => panic!("expected puts"),
            })
            .collect();
        assert_eq!(revs, vec![1, 2]);
        assert_eq!(resp.revision, 2);
    }
}
