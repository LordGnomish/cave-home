// SPDX-License-Identifier: Apache-2.0
//! Lease-based leader election.
//!
//! Behavioural reimplementation of the documented
//! `client-go/tools/leaderelection` contract over a `coordination.k8s.io/v1`
//! [`Lease`]: a single active controller-manager among many replicas. The pure
//! decision is [`try_acquire_or_renew`]:
//!
//! * **no lease** → the caller creates it and becomes leader;
//! * **the caller already holds it** → renew (advance `renew_time`, keep
//!   `acquire_time`, no transition);
//! * **another holder, lease expired** (`now >= renew_time + duration`) → take
//!   over, bumping `lease_transitions` and resetting `acquire_time`;
//! * **another holder, lease still valid** → the caller has lost the election.
//!
//! `std` only; time is a caller-supplied `now` (epoch seconds), never a clock
//! read — identical to the rest of this crate. The networked Lease read/write
//! (with optimistic `resourceVersion` concurrency) is deferred; the
//! single-writer decision is exactly what benefits from exhaustive testing.

/// A coordination Lease (`coordination.k8s.io/v1` `LeaseSpec` subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    /// Identity of the current holder (`spec.holderIdentity`).
    pub holder_identity: String,
    /// How long (seconds) a holder's claim is valid past its last renewal
    /// (`spec.leaseDurationSeconds`).
    pub lease_duration_secs: i64,
    /// Epoch-seconds the current holder *first* acquired leadership
    /// (`spec.acquireTime`).
    pub acquire_time: i64,
    /// Epoch-seconds of the holder's most recent renewal (`spec.renewTime`).
    pub renew_time: i64,
    /// Number of times leadership has changed hands (`spec.leaseTransitions`).
    pub lease_transitions: i64,
}

impl Lease {
    /// `true` if the lease is still valid at `now` (not past `renew + duration`).
    #[must_use]
    pub const fn is_valid(&self, now: i64) -> bool {
        now < self.renew_time + self.lease_duration_secs
    }
}

/// The outcome of an [`try_acquire_or_renew`] attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElectionResult {
    /// The caller became leader (new lease or a takeover); carries the lease to
    /// persist.
    AcquiredLeadership(Lease),
    /// The caller already led and renewed; carries the updated lease.
    RenewedLeadership(Lease),
    /// The caller is not leader: another identity holds a still-valid lease.
    Lost {
        /// The identity currently holding the valid lease.
        current_holder: String,
    },
}

/// The pure leader-election decision over the current lease state.
///
/// Returns the result and, for the two leadership outcomes, the lease the
/// caller should write back. Does not mutate anything.
#[must_use]
pub fn try_acquire_or_renew(
    current: Option<&Lease>,
    identity: &str,
    now: i64,
    lease_duration_secs: i64,
) -> ElectionResult {
    let Some(lease) = current else {
        // No lease exists: create one and lead.
        return ElectionResult::AcquiredLeadership(Lease {
            holder_identity: identity.to_owned(),
            lease_duration_secs,
            acquire_time: now,
            renew_time: now,
            lease_transitions: 0,
        });
    };

    if lease.holder_identity == identity {
        // We already lead: renew. acquire_time and transitions are preserved.
        let mut renewed = lease.clone();
        renewed.renew_time = now;
        renewed.lease_duration_secs = lease_duration_secs;
        return ElectionResult::RenewedLeadership(renewed);
    }

    // A released lease (empty holder) is available regardless of its renew_time;
    // a still-valid lease held by another locks us out.
    if !lease.holder_identity.is_empty() && lease.is_valid(now) {
        // Someone else holds a valid lease.
        return ElectionResult::Lost { current_holder: lease.holder_identity.clone() };
    }

    // Expired, or cleanly released, and held by another: take over, counting the
    // transition.
    ElectionResult::AcquiredLeadership(Lease {
        holder_identity: identity.to_owned(),
        lease_duration_secs,
        acquire_time: now,
        renew_time: now,
        lease_transitions: lease.lease_transitions + 1,
    })
}

/// One participant in the election, identified by `identity`, claiming leases of
/// `lease_duration` seconds.
#[derive(Debug, Clone)]
pub struct LeaderElector {
    identity: String,
    lease_duration: i64,
}

impl LeaderElector {
    /// A participant with the given identity and lease duration (seconds).
    #[must_use]
    pub fn new(identity: &str, lease_duration: i64) -> Self {
        Self { identity: identity.to_owned(), lease_duration }
    }

    /// This participant's identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Attempt to acquire or renew leadership against the shared `lease` slot
    /// (the single Lease object the apiserver would hold). On either leadership
    /// outcome the new lease is written back into `lease`, modelling the
    /// apiserver update; a [`ElectionResult::Lost`] leaves it untouched.
    pub fn try_acquire_or_renew(&self, lease: &mut Option<Lease>, now: i64) -> ElectionResult {
        let result = try_acquire_or_renew(lease.as_ref(), &self.identity, now, self.lease_duration);
        match &result {
            ElectionResult::AcquiredLeadership(l) | ElectionResult::RenewedLeadership(l) => {
                *lease = Some(l.clone());
            }
            ElectionResult::Lost { .. } => {}
        }
        result
    }

    /// `true` if this participant currently holds a valid lease at `now`.
    #[must_use]
    pub fn is_leader(&self, lease: &Option<Lease>, now: i64) -> bool {
        lease
            .as_ref()
            .is_some_and(|l| l.holder_identity == self.identity && l.is_valid(now))
    }

    /// Gracefully relinquish leadership (the `release` half of `RunOrDie`'s
    /// deferred cleanup): if this participant currently holds the lease, clear
    /// its `holder_identity` so a successor can acquire **immediately** rather
    /// than waiting out the full lease duration. Returns `true` if a release
    /// happened, `false` if this participant did not hold the lease.
    pub fn release(&self, lease: &mut Option<Lease>, now: i64) -> bool {
        let Some(l) = lease.as_mut() else { return false };
        if l.holder_identity != self.identity {
            return false;
        }
        l.holder_identity.clear();
        l.renew_time = now;
        true
    }

    /// Record an observation of the current lease record at `now`. The returned
    /// [`ObservedRecord`] timestamps **when this participant first saw** the
    /// present record — the basis for detecting a stalled leader independently
    /// of the holder's own clock.
    #[must_use]
    pub fn observe(&self, lease: Option<&Lease>, now: i64) -> ObservedRecord {
        ObservedRecord { record: lease.cloned(), observed_at: now }
    }

    /// Fold a fresh sighting into a prior [`ObservedRecord`]: if the lease record
    /// is byte-for-byte unchanged, the original `observed_at` is preserved;
    /// otherwise the clock resets to `now` (the record just changed).
    #[must_use]
    pub fn observe_again(
        &self,
        prior: ObservedRecord,
        lease: Option<&Lease>,
        now: i64,
    ) -> ObservedRecord {
        if prior.record.as_ref() == lease {
            prior
        } else {
            ObservedRecord { record: lease.cloned(), observed_at: now }
        }
    }
}

/// A watcher's view of the lease: the last record it saw and the instant it
/// first saw that record (`LeaderElectionRecord` + `observedTime`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedRecord {
    /// The lease record as last observed (`None` if no lease existed).
    pub record: Option<Lease>,
    /// Epoch-seconds this participant first observed the current `record`.
    pub observed_at: i64,
}

/// Timing configuration for the leader-election loop
/// (`client-go` `LeaderElectionConfig`): the three durations that govern how a
/// holder renews and how a challenger waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderElectionConfig {
    /// How long a lease is honoured past its last renewal (`LeaseDuration`).
    pub lease_duration: i64,
    /// How long a holder keeps retrying to renew before giving up leadership
    /// (`RenewDeadline`). Must be `< lease_duration`.
    pub renew_deadline: i64,
    /// The gap between a participant's election attempts (`RetryPeriod`). Must be
    /// `< renew_deadline`.
    pub retry_period: i64,
}

impl LeaderElectionConfig {
    /// Build a config, enforcing the upstream ordering invariant
    /// `lease_duration > renew_deadline > retry_period` and
    /// `renew_deadline > 0`.
    ///
    /// # Errors
    /// Returns a static message if the ordering invariant is violated.
    pub const fn new(
        lease_duration: i64,
        renew_deadline: i64,
        retry_period: i64,
    ) -> Result<Self, &'static str> {
        if renew_deadline <= 0 {
            return Err("renew_deadline must be > 0");
        }
        if lease_duration <= renew_deadline {
            return Err("lease_duration must be greater than renew_deadline");
        }
        if renew_deadline <= retry_period {
            return Err("renew_deadline must be greater than retry_period");
        }
        Ok(Self { lease_duration, renew_deadline, retry_period })
    }

    /// `true` if a holder whose last **successful** renewal was at
    /// `last_renew` has blown its renew deadline by `now` and must stop acting
    /// as leader (`tryAcquireOrRenew`'s deadline guard). Stopping early — before
    /// the lease formally expires from a challenger's view — is what keeps two
    /// instances from both believing they lead.
    #[must_use]
    pub const fn renew_deadline_exceeded(&self, last_renew: i64, now: i64) -> bool {
        now - last_renew > self.renew_deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_lease_is_acquired() {
        let r = try_acquire_or_renew(None, "x", 0, 10);
        assert!(matches!(r, ElectionResult::AcquiredLeadership(l) if l.holder_identity == "x"));
    }

    #[test]
    fn valid_lease_held_by_other_is_lost() {
        let l = Lease {
            holder_identity: "other".into(),
            lease_duration_secs: 10,
            acquire_time: 0,
            renew_time: 0,
            lease_transitions: 0,
        };
        assert_eq!(
            try_acquire_or_renew(Some(&l), "me", 5, 10),
            ElectionResult::Lost { current_holder: "other".into() }
        );
    }

    #[test]
    fn is_valid_boundary() {
        let l = Lease {
            holder_identity: "a".into(),
            lease_duration_secs: 10,
            acquire_time: 0,
            renew_time: 100,
            lease_transitions: 0,
        };
        assert!(l.is_valid(109));
        assert!(!l.is_valid(110), "expires exactly at renew+duration");
    }
}
