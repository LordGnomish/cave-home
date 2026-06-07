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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::range::RangeEnd;
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
