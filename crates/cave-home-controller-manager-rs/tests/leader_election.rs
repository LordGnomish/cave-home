// SPDX-License-Identifier: Apache-2.0
//! Multi-instance leader election over a shared coordination Lease
//! (`client-go/tools/leaderelection` + `coordination.k8s.io/v1` Lease).

use cave_home_controller_manager_rs::leaderelection::{ElectionResult, Lease, LeaderElector};

const DURATION: i64 = 15;

#[test]
fn first_caller_acquires_an_empty_lease() {
    let a = LeaderElector::new("A", DURATION);
    let mut lease: Option<Lease> = None;
    match a.try_acquire_or_renew(&mut lease, 100) {
        ElectionResult::AcquiredLeadership(l) => {
            assert_eq!(l.holder_identity, "A");
            assert_eq!(l.acquire_time, 100);
            assert_eq!(l.renew_time, 100);
            assert_eq!(l.lease_transitions, 0);
        }
        other => panic!("expected acquisition, got {other:?}"),
    }
    assert!(a.is_leader(&lease, 100));
}

#[test]
fn second_caller_is_rejected_while_the_lease_is_valid() {
    let a = LeaderElector::new("A", DURATION);
    let b = LeaderElector::new("B", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100); // A leads

    match b.try_acquire_or_renew(&mut lease, 105) {
        ElectionResult::Lost { current_holder } => assert_eq!(current_holder, "A"),
        other => panic!("B must not steal a valid lease, got {other:?}"),
    }
    assert!(!b.is_leader(&lease, 105));
    assert!(a.is_leader(&lease, 105), "A still holds it");
    // The lease object was not mutated by B's failed attempt.
    assert_eq!(lease.as_ref().unwrap().holder_identity, "A");
}

#[test]
fn holder_renews_and_extends_validity() {
    let a = LeaderElector::new("A", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100);

    match a.try_acquire_or_renew(&mut lease, 110) {
        ElectionResult::RenewedLeadership(l) => {
            assert_eq!(l.renew_time, 110, "renew time advanced");
            assert_eq!(l.acquire_time, 100, "acquire time unchanged on renew");
            assert_eq!(l.lease_transitions, 0, "renew is not a transition");
        }
        other => panic!("expected renewal, got {other:?}"),
    }
    // Validity now extends to 110 + DURATION.
    assert!(a.is_leader(&lease, 124));
    assert!(!a.is_leader(&lease, 125), "lease expires at renew+duration");
}

#[test]
fn expired_lease_is_taken_over_and_increments_transitions() {
    let a = LeaderElector::new("A", DURATION);
    let b = LeaderElector::new("B", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100); // A leads, valid until 115

    // A goes silent; B retries after expiry (>= 115).
    match b.try_acquire_or_renew(&mut lease, 116) {
        ElectionResult::AcquiredLeadership(l) => {
            assert_eq!(l.holder_identity, "B");
            assert_eq!(l.acquire_time, 116);
            assert_eq!(l.renew_time, 116);
            assert_eq!(l.lease_transitions, 1, "leadership changed hands once");
        }
        other => panic!("B should take over an expired lease, got {other:?}"),
    }
    assert!(b.is_leader(&lease, 116));
    assert!(!a.is_leader(&lease, 116), "A lost leadership");
}

#[test]
fn original_holder_is_locked_out_after_a_takeover() {
    let a = LeaderElector::new("A", DURATION);
    let b = LeaderElector::new("B", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100);
    b.try_acquire_or_renew(&mut lease, 200); // takes over (expired)

    // A comes back while B's fresh lease is valid -> Lost.
    match a.try_acquire_or_renew(&mut lease, 205) {
        ElectionResult::Lost { current_holder } => assert_eq!(current_holder, "B"),
        other => panic!("A must wait out B's valid lease, got {other:?}"),
    }
}

#[test]
fn at_most_one_leader_under_contention() {
    let a = LeaderElector::new("A", DURATION);
    let b = LeaderElector::new("B", DURATION);
    let mut lease: Option<Lease> = None;
    // Both race at the same instant; whoever writes first wins, the other loses.
    a.try_acquire_or_renew(&mut lease, 0);
    b.try_acquire_or_renew(&mut lease, 0);
    let now = 5;
    let leaders = [a.is_leader(&lease, now), b.is_leader(&lease, now)];
    assert_eq!(leaders.iter().filter(|x| **x).count(), 1, "exactly one leader at any time");
}
