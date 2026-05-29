//! Active-passive failover — the core decision engine.
//!
//! When the primary hub goes out of touch and a backup hub is healthy, cave-home
//! promotes the backup so the home keeps automating (Charter §5). But promotion
//! is dangerous: if we promote a backup while the old primary is *actually still
//! running* — we just lost contact with it — we get **two active primaries**
//! (split-brain), and two hubs fighting over the radios and broker is worse than
//! a brief outage.
//!
//! The guard is a **fencing gate**. Before promoting, the cluster must
//! *confirm* the old primary is really down (powered off / isolated). This
//! crate does not implement fencing (a real STONITH-class mechanism is Phase 1b,
//! network/power-bound, see the parity manifest); it models the *decision*:
//! given a [`FenceStatus`], do we promote or do we hold?
//!
//! - [`FenceStatus::Confirmed`] — the old primary is provably down → promote.
//! - [`FenceStatus::Unconfirmed`] — we could not confirm → **hold** (refuse to
//!   risk split-brain), report that we are blocked on fencing.
//! - [`FenceStatus::NotNeeded`] — there is no live primary to fence against
//!   (e.g. cold start) → promote.
//!
//! When the old primary later returns, [`FailbackPolicy`] decides whether it
//! reclaims the role automatically or waits for a human.

use crate::node::{NodeHealth, NodeRole};
use crate::topology::Cluster;

/// The result of the fencing gate the caller supplies to the failover decision.
///
/// "Fencing" = making sure the old primary cannot keep acting as primary (power
/// it off, cut its access to the radios). The actual mechanism is Phase 1b; the
/// caller passes the *outcome* here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceStatus {
    /// The old primary is confirmed down / isolated. Safe to promote.
    Confirmed,
    /// Fencing could not confirm the old primary is down. We must NOT promote —
    /// promoting now risks two active primaries (split-brain).
    Unconfirmed,
    /// There is no live primary that needs fencing (cold start, or the only
    /// primary-capable node left is the backup). Safe to promote.
    NotNeeded,
}

/// What to do when a former primary comes back after a failover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailbackPolicy {
    /// A human confirms before the original primary reclaims the role. This is
    /// the safe default: an automatic flap-back during a flaky link can cause
    /// repeated promotion churn.
    #[default]
    Manual,
    /// The original primary automatically reclaims the role once it is healthy
    /// again. Convenient, but only sound on stable hardware.
    Automatic,
}

/// The decision the failover engine reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailoverPlan {
    /// Nothing to do — the primary is fine (or there is no actionable change).
    NoAction,
    /// Promote `node` (a backup hub) to the active primary. `demote` names the
    /// old primary to step down, when one exists.
    Promote { node: String, demote: Option<String> },
    /// The primary is down and a backup is ready, but fencing could not confirm
    /// the old primary is really gone. We are holding to avoid split-brain. The
    /// homeowner is told "checking on the main hub", never the word "fencing".
    BlockedOnFencing { candidate: String },
    /// The primary is down but there is no healthy backup to take over. The
    /// home is degraded; a human must intervene.
    NoHealthyBackup,
    /// A former primary has returned healthy and failback is automatic — hand
    /// the role back to it and step the current acting primary down.
    Failback { to: String, demote: Option<String> },
}

impl FailoverPlan {
    /// Whether this plan actually changes the active primary.
    #[must_use]
    pub const fn changes_primary(&self) -> bool {
        matches!(self, Self::Promote { .. } | Self::Failback { .. })
    }
}

/// Decide the failover action for a cluster, given a fencing result.
///
/// Expects health to have been refreshed for the relevant tick already (the
/// [`Cluster::decide_failover`](crate::topology::Cluster::decide_failover)
/// wrapper does this for you). The logic:
///
/// 1. If there is exactly one active primary and it is healthy → [`FailoverPlan::NoAction`].
/// 2. If the active primary is unreachable (or absent):
///    - no healthy backup → [`FailoverPlan::NoHealthyBackup`];
///    - healthy backup but fencing [`FenceStatus::Unconfirmed`] →
///      [`FailoverPlan::BlockedOnFencing`] (split-brain guard);
///    - healthy backup and fencing confirmed / not-needed → [`FailoverPlan::Promote`].
/// 3. A degraded (but reachable) primary is *not* failed over — degraded means
///    "still in touch", and promoting over a live primary is the split-brain we
///    are guarding against. Wait for it to recover or become unreachable.
#[must_use]
pub fn decide_failover(cluster: &Cluster, fence: FenceStatus) -> FailoverPlan {
    let primary = cluster.active_primary();

    // A single primary that is still reachable: nothing to do. Degraded means
    // "late heartbeats but still in touch" — promoting over a live primary is
    // exactly the split-brain we guard against, so we hold for Healthy AND
    // Degraded and only act once the primary is Unreachable.
    if let Some(p) = primary {
        match p.health() {
            NodeHealth::Healthy | NodeHealth::Degraded => return FailoverPlan::NoAction,
            NodeHealth::Unreachable => { /* fall through to failover logic */ }
        }
    }

    // Primary is unreachable or there is no single active primary: consider a
    // backup.
    let Some(backup) = cluster.healthy_backup() else {
        return FailoverPlan::NoHealthyBackup;
    };
    let candidate = backup.id().to_owned();
    let demote = primary.map(|p| p.id().to_owned());

    match fence {
        FenceStatus::Confirmed | FenceStatus::NotNeeded => {
            FailoverPlan::Promote { node: candidate, demote }
        }
        // Split-brain guard: refuse to promote when we cannot confirm the old
        // primary is gone.
        FenceStatus::Unconfirmed => FailoverPlan::BlockedOnFencing { candidate },
    }
}

/// Decide a failback when a former primary (`returning_id`) has come back.
///
/// `acting_primary` is the node currently serving as primary (usually the
/// promoted backup). The returning node is assumed primary-capable and healthy
/// by the time this is called.
///
/// - [`FailbackPolicy::Manual`] → [`FailoverPlan::NoAction`]: stay on the
///   acting primary until a human says otherwise.
/// - [`FailbackPolicy::Automatic`] → [`FailoverPlan::Failback`]: hand the role
///   back, demoting the acting primary.
#[must_use]
pub fn decide_failback(
    policy: FailbackPolicy,
    returning_id: &str,
    acting_primary: Option<&str>,
) -> FailoverPlan {
    match policy {
        FailbackPolicy::Manual => FailoverPlan::NoAction,
        FailbackPolicy::Automatic => FailoverPlan::Failback {
            to: returning_id.to_owned(),
            demote: acting_primary.map(str::to_owned),
        },
    }
}

/// Convenience: is `role` the role we would promote *to* during failover?
#[must_use]
pub const fn is_promotion_target(role: NodeRole) -> bool {
    matches!(role, NodeRole::BackupHub)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn two_hub_cluster() -> Cluster {
        let mut c = Cluster::new();
        c.add(Node::new("hub-1", "Main hub", NodeRole::Primary).with_radios());
        c.add(Node::new("hub-2", "Backup hub", NodeRole::BackupHub).with_radios());
        c
    }

    #[test]
    fn healthy_primary_no_action() {
        let mut c = two_hub_cluster();
        c.set_heartbeat("hub-1", 1000);
        c.set_heartbeat("hub-2", 1000);
        let plan = c.decide_failover(1000, FenceStatus::Confirmed);
        assert_eq!(plan, FailoverPlan::NoAction);
    }

    #[test]
    fn primary_down_backup_healthy_fence_confirmed_promotes() {
        let mut c = two_hub_cluster();
        c.set_heartbeat("hub-1", 100); // far stale -> unreachable
        c.set_heartbeat("hub-2", 998); // fresh
        let plan = c.decide_failover(1000, FenceStatus::Confirmed);
        assert_eq!(
            plan,
            FailoverPlan::Promote {
                node: "hub-2".to_owned(),
                demote: Some("hub-1".to_owned()),
            }
        );
        assert!(plan.changes_primary());
    }

    #[test]
    fn split_brain_guard_holds_when_fence_unconfirmed() {
        let mut c = two_hub_cluster();
        c.set_heartbeat("hub-1", 100); // primary appears down...
        c.set_heartbeat("hub-2", 998);
        // ...but we cannot CONFIRM it is really down. Must not promote.
        let plan = c.decide_failover(1000, FenceStatus::Unconfirmed);
        assert_eq!(
            plan,
            FailoverPlan::BlockedOnFencing { candidate: "hub-2".to_owned() }
        );
        assert!(!plan.changes_primary());
    }

    #[test]
    fn primary_down_no_healthy_backup() {
        let mut c = two_hub_cluster();
        c.set_heartbeat("hub-1", 100); // down
        c.set_heartbeat("hub-2", 100); // backup also down
        let plan = c.decide_failover(1000, FenceStatus::Confirmed);
        assert_eq!(plan, FailoverPlan::NoHealthyBackup);
    }

    #[test]
    fn degraded_primary_is_not_failed_over() {
        // age between degraded and unreachable thresholds (default 5/15).
        let mut c = two_hub_cluster();
        c.set_heartbeat("hub-1", 990); // age 10 -> degraded (still reachable)
        c.set_heartbeat("hub-2", 1000);
        let plan = c.decide_failover(1000, FenceStatus::Confirmed);
        assert_eq!(plan, FailoverPlan::NoAction);
    }

    #[test]
    fn cold_start_lone_backup_promotes_with_fence_not_needed() {
        // No live primary at all: a lone healthy backup is promoted; there is
        // nothing to fence.
        let mut c = Cluster::new();
        c.add(Node::new("hub-2", "Backup hub", NodeRole::BackupHub));
        c.set_heartbeat("hub-2", 1000);
        let plan = c.decide_failover(1000, FenceStatus::NotNeeded);
        assert_eq!(
            plan,
            FailoverPlan::Promote { node: "hub-2".to_owned(), demote: None }
        );
    }

    #[test]
    fn manual_failback_is_no_action() {
        let plan = decide_failback(FailbackPolicy::Manual, "hub-1", Some("hub-2"));
        assert_eq!(plan, FailoverPlan::NoAction);
    }

    #[test]
    fn automatic_failback_hands_role_back() {
        let plan = decide_failback(FailbackPolicy::Automatic, "hub-1", Some("hub-2"));
        assert_eq!(
            plan,
            FailoverPlan::Failback {
                to: "hub-1".to_owned(),
                demote: Some("hub-2".to_owned()),
            }
        );
        assert!(plan.changes_primary());
    }

    #[test]
    fn default_failback_policy_is_manual() {
        assert_eq!(FailbackPolicy::default(), FailbackPolicy::Manual);
    }

    #[test]
    fn promotion_target_is_backup_only() {
        assert!(is_promotion_target(NodeRole::BackupHub));
        assert!(!is_promotion_target(NodeRole::Primary));
        assert!(!is_promotion_target(NodeRole::MlGpu));
    }

    #[test]
    fn end_to_end_promote_then_topology_valid() {
        let mut c = two_hub_cluster();
        c.set_heartbeat("hub-1", 100);
        c.set_heartbeat("hub-2", 1000);
        let plan = c.decide_failover(1000, FenceStatus::Confirmed);
        assert!(c.apply_failover(&plan));
        // After promotion the new sole primary is hub-2 and the topology holds.
        assert_eq!(c.active_primaries(), vec!["hub-2"]);
        assert!(c.validate().is_ok());
    }
}
