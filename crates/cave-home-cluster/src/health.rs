//! Heartbeat-age health classification.
//!
//! cave-home does not have a real clock in this crate (Charter: pure logic, no
//! time crate). The caller supplies a monotonic *tick* — `now` — and each node
//! carries the tick of its last heartbeat. Health is purely a function of how
//! old that heartbeat is against two thresholds:
//!
//! - within `degraded_after` ticks → [`NodeHealth::Healthy`]
//! - within `unreachable_after` ticks → [`NodeHealth::Degraded`]
//! - older than that (or never heard from) → [`NodeHealth::Unreachable`]
//!
//! Ticks are an abstract unit; a deployment maps them to seconds (or to a
//! heartbeat sequence number) when it wires up the transport in Phase 1b.

use crate::node::NodeHealth;

/// The two age thresholds that split healthy / degraded / unreachable.
///
/// Invariant: `degraded_after <= unreachable_after`. The constructor enforces
/// it; the [`Default`] is a sane example (a node is degraded after a few missed
/// beats and presumed down after several).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthThresholds {
    degraded_after: u64,
    unreachable_after: u64,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        // Example mapping: heartbeat ~every tick; degraded after 5 missed,
        // presumed down after 15. The transport layer (Phase 1b) picks the real
        // numbers; these keep the engine testable and give a sensible fallback.
        Self { degraded_after: 5, unreachable_after: 15 }
    }
}

impl HealthThresholds {
    /// Build thresholds, clamping so the invariant `degraded <= unreachable`
    /// always holds (a caller cannot construct a nonsensical pair).
    #[must_use]
    pub const fn new(degraded_after: u64, unreachable_after: u64) -> Self {
        let unreachable_after = if unreachable_after < degraded_after {
            degraded_after
        } else {
            unreachable_after
        };
        Self { degraded_after, unreachable_after }
    }

    #[must_use]
    pub const fn degraded_after(self) -> u64 {
        self.degraded_after
    }

    #[must_use]
    pub const fn unreachable_after(self) -> u64 {
        self.unreachable_after
    }
}

/// Classify health from the last-heartbeat tick and the current tick.
///
/// A node never heard from (`last_heartbeat == None`) is [`NodeHealth::Unreachable`].
/// A heartbeat *newer* than `now` (clock skew / out-of-order delivery) is
/// treated as age zero rather than panicking on the subtraction.
#[must_use]
pub const fn classify_health(
    last_heartbeat: Option<u64>,
    now: u64,
    thresholds: HealthThresholds,
) -> NodeHealth {
    let Some(last) = last_heartbeat else {
        return NodeHealth::Unreachable;
    };
    // Saturating: a future-dated heartbeat clamps to age 0 (not a panic).
    let age = now.saturating_sub(last);
    if age <= thresholds.degraded_after {
        NodeHealth::Healthy
    } else if age <= thresholds.unreachable_after {
        NodeHealth::Degraded
    } else {
        NodeHealth::Unreachable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: HealthThresholds = HealthThresholds::new(5, 15);

    #[test]
    fn never_heard_from_is_unreachable() {
        assert_eq!(classify_health(None, 1000, T), NodeHealth::Unreachable);
    }

    #[test]
    fn fresh_heartbeat_is_healthy() {
        assert_eq!(classify_health(Some(1000), 1000, T), NodeHealth::Healthy);
        assert_eq!(classify_health(Some(996), 1000, T), NodeHealth::Healthy);
    }

    #[test]
    fn healthy_degraded_boundary_is_inclusive() {
        // age 5 == degraded_after -> still healthy (inclusive lower band).
        assert_eq!(classify_health(Some(995), 1000, T), NodeHealth::Healthy);
        // age 6 -> degraded.
        assert_eq!(classify_health(Some(994), 1000, T), NodeHealth::Degraded);
    }

    #[test]
    fn degraded_unreachable_boundary_is_inclusive() {
        // age 15 == unreachable_after -> still degraded.
        assert_eq!(classify_health(Some(985), 1000, T), NodeHealth::Degraded);
        // age 16 -> unreachable.
        assert_eq!(classify_health(Some(984), 1000, T), NodeHealth::Unreachable);
    }

    #[test]
    fn far_stale_heartbeat_is_unreachable() {
        assert_eq!(classify_health(Some(0), 1000, T), NodeHealth::Unreachable);
    }

    #[test]
    fn future_heartbeat_clamps_to_healthy_not_panic() {
        // last > now (out-of-order / skew): age saturates to 0 -> healthy.
        assert_eq!(classify_health(Some(1010), 1000, T), NodeHealth::Healthy);
    }

    #[test]
    fn thresholds_clamp_to_invariant() {
        // unreachable < degraded gets clamped up so the bands never invert.
        let bad = HealthThresholds::new(20, 5);
        assert_eq!(bad.degraded_after(), 20);
        assert_eq!(bad.unreachable_after(), 20);
    }
}
