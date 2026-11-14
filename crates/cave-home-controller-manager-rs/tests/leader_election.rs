// SPDX-License-Identifier: Apache-2.0
//! Multi-instance leader election over a shared coordination Lease
//! (`client-go/tools/leaderelection` + `coordination.k8s.io/v1` Lease).

use cave_home_controller_manager_rs::leaderelection::{ElectionResult, LeaderElector, Lease};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DURATION: i64 = 15;

#[test]
fn first_caller_acquires_an_empty_lease() -> TestResult {
    let a = LeaderElector::new("A", DURATION);
    let mut lease: Option<Lease> = None;
    match a.try_acquire_or_renew(&mut lease, 100) {
        ElectionResult::AcquiredLeadership(l) => {
            assert_eq!(l.holder_identity, "A");
            assert_eq!(l.acquire_time, 100);
            assert_eq!(l.renew_time, 100);
            assert_eq!(l.lease_transitions, 0);
        }
        other => return Err(format!("expected acquisition, got {other:?}").into()),
    }
    assert!(a.is_leader(&lease, 100));
    Ok(())
}

#[test]
fn second_caller_is_rejected_while_the_lease_is_valid() -> TestResult {
    let a = LeaderElector::new("A", DURATION);
    let b = LeaderElector::new("B", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100); // A leads

    match b.try_acquire_or_renew(&mut lease, 105) {
        ElectionResult::Lost { current_holder } => assert_eq!(current_holder, "A"),
        other => return Err(format!("B must not steal a valid lease, got {other:?}").into()),
    }
    assert!(!b.is_leader(&lease, 105));
    assert!(a.is_leader(&lease, 105), "A still holds it");
    // The lease object was not mutated by B's failed attempt.
    assert_eq!(
        lease
            .as_ref()
            .ok_or("expected Some, got None")?
            .holder_identity,
        "A"
    );
    Ok(())
}

#[test]
fn holder_renews_and_extends_validity() -> TestResult {
    let a = LeaderElector::new("A", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100);

    match a.try_acquire_or_renew(&mut lease, 110) {
        ElectionResult::RenewedLeadership(l) => {
            assert_eq!(l.renew_time, 110, "renew time advanced");
            assert_eq!(l.acquire_time, 100, "acquire time unchanged on renew");
            assert_eq!(l.lease_transitions, 0, "renew is not a transition");
        }
        other => return Err(format!("expected renewal, got {other:?}").into()),
    }
    // Validity now extends to 110 + DURATION.
    assert!(a.is_leader(&lease, 124));
    assert!(!a.is_leader(&lease, 125), "lease expires at renew+duration");
    Ok(())
}

#[test]
fn expired_lease_is_taken_over_and_increments_transitions() -> TestResult {
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
        other => return Err(format!("B should take over an expired lease, got {other:?}").into()),
    }
    assert!(b.is_leader(&lease, 116));
    assert!(!a.is_leader(&lease, 116), "A lost leadership");
    Ok(())
}

#[test]
fn original_holder_is_locked_out_after_a_takeover() -> TestResult {
    let a = LeaderElector::new("A", DURATION);
    let b = LeaderElector::new("B", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100);
    b.try_acquire_or_renew(&mut lease, 200); // takes over (expired)

    // A comes back while B's fresh lease is valid -> Lost.
    match a.try_acquire_or_renew(&mut lease, 205) {
        ElectionResult::Lost { current_holder } => assert_eq!(current_holder, "B"),
        other => return Err(format!("A must wait out B's valid lease, got {other:?}").into()),
    }
    Ok(())
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
    assert_eq!(
        leaders.iter().filter(|x| **x).count(),
        1,
        "exactly one leader at any time"
    );
}

// --- Graceful release ---------------------------------------------------

#[test]
fn release_lets_a_successor_acquire_immediately_without_waiting_for_expiry() -> TestResult {
    let a = LeaderElector::new("A", DURATION);
    let b = LeaderElector::new("B", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100); // A leads, valid until 115

    // A releases on graceful shutdown at t=103 (well before expiry).
    assert!(
        a.release(&mut lease, 103),
        "the holder releases its own lease"
    );
    assert!(
        !a.is_leader(&lease, 103),
        "A is no longer leader after releasing"
    );

    // B can acquire right away even though the original duration had not elapsed.
    match b.try_acquire_or_renew(&mut lease, 104) {
        ElectionResult::AcquiredLeadership(l) => {
            assert_eq!(l.holder_identity, "B");
            // A released cleanly, so the takeover still counts as a transition.
            assert_eq!(l.lease_transitions, 1);
        }
        other => {
            return Err(
                format!("B should acquire a released lease immediately, got {other:?}").into(),
            );
        }
    }
    Ok(())
}

#[test]
fn a_non_holder_cannot_release_the_lease() {
    let a = LeaderElector::new("A", DURATION);
    let b = LeaderElector::new("B", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100);
    assert!(
        !b.release(&mut lease, 101),
        "B does not hold the lease, so cannot release it"
    );
    assert!(a.is_leader(&lease, 101), "A still holds it");
}

#[test]
fn released_lease_is_expired_for_validity_checks() -> TestResult {
    let a = LeaderElector::new("A", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100);
    a.release(&mut lease, 102);
    // An empty-holder lease is held by nobody.
    assert!(!a.is_leader(&lease, 102));
    assert!(
        lease
            .as_ref()
            .ok_or("expected Some, got None")?
            .holder_identity
            .is_empty(),
        "holder cleared on release"
    );
    Ok(())
}

// --- Renew-deadline timing ----------------------------------------------

use cave_home_controller_manager_rs::leaderelection::LeaderElectionConfig;

#[test]
fn config_rejects_a_renew_deadline_that_is_not_shorter_than_the_lease_duration() {
    // Upstream invariant: leaseDuration > renewDeadline > 0 and
    // renewDeadline > retryPeriod*JitterFactor; we enforce the ordering.
    assert!(LeaderElectionConfig::new(15, 10, 2).is_ok());
    assert!(
        LeaderElectionConfig::new(10, 10, 2).is_err(),
        "renew_deadline must be < lease_duration"
    );
    assert!(
        LeaderElectionConfig::new(15, 2, 5).is_err(),
        "retry_period must be < renew_deadline"
    );
    assert!(
        LeaderElectionConfig::new(15, 0, 0).is_err(),
        "renew_deadline must be > 0"
    );
}

#[test]
fn a_holder_that_misses_its_renew_deadline_must_stop_leading() -> TestResult {
    // A leads at t=100. It keeps trying to renew but the writes never land
    // (e.g. apiserver unreachable): once now - last_successful_renew exceeds the
    // renew deadline, the elector reports it has lost leadership and must stop
    // acting, even though from another instance's view the lease has not yet
    // expired.
    let cfg = LeaderElectionConfig::new(15, 10, 2)?;
    let a = LeaderElector::new("A", cfg.lease_duration);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100); // last successful renew = 100
    let last_renew = 100;

    // 9s after the last successful renew: still within the 10s deadline.
    assert!(
        !cfg.renew_deadline_exceeded(last_renew, 109),
        "within deadline, keep leading"
    );
    // 11s after: deadline blown, must relinquish.
    assert!(
        cfg.renew_deadline_exceeded(last_renew, 111),
        "deadline exceeded, stop leading"
    );
    Ok(())
}

// --- Observed-record tracking -------------------------------------------

#[test]
fn observe_tracks_the_time_the_record_last_changed() {
    let a = LeaderElector::new("A", DURATION);
    let b = LeaderElector::new("B", DURATION);
    let mut lease: Option<Lease> = None;
    a.try_acquire_or_renew(&mut lease, 100);

    // B watches the lease. It records *when it first saw* the current record.
    let mut obs = b.observe(lease.as_ref(), 105);
    assert_eq!(obs.observed_at, 105, "first observation time");

    // The record is unchanged at t=108: observed_at stays put.
    obs = b.observe_again(obs, lease.as_ref(), 108);
    assert_eq!(
        obs.observed_at, 105,
        "unchanged record keeps the original observation time"
    );

    // A renews at 110 (record changes); B's next observation resets the clock.
    a.try_acquire_or_renew(&mut lease, 110);
    obs = b.observe_again(obs, lease.as_ref(), 112);
    assert_eq!(obs.observed_at, 112, "a changed record resets observed_at");
}
