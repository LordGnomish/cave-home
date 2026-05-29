// SPDX-License-Identifier: Apache-2.0
//! The reconcile decision core: desired `HelmChart` vs observed release state
//! → the action to take.
//!
//! Behavioural reimplementation of helm-controller's reconcile loop. We model
//! the *decision* only; creating the Job, watching it, and writing status back
//! to the cluster are deferred (Phase 1b). The decision is a pure function of
//! the desired spec hash and the observed release, which makes it fully
//! testable without a cluster.
//!
//! Spec sources (public, Apache-2.0-compatible documentation):
//! * k3s-io/helm-controller public docs (install-on-create, upgrade-on-change,
//!   uninstall-on-delete, job-driven apply).
//! * Helm release lifecycle (`deployed` / `failed` / `pending-*` statuses,
//!   `helm rollback` on a failed upgrade).

use crate::chart::HelmChart;
use crate::values::Value;

/// Observed state of the Helm release backing a `HelmChart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseState {
    /// No release exists yet for this chart.
    Absent,
    /// A release is deployed; `applied_hash` is the hash recorded for it and
    /// `revision` its helm revision counter.
    Deployed { applied_hash: String, revision: u32 },
    /// The most recent upgrade attempt failed; the release is in a failed
    /// state. `last_good_revision` is the revision to roll back to.
    Failed {
        applied_hash: String,
        last_good_revision: u32,
    },
}

/// The reconcile action chosen for one pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No existing release → install.
    Install,
    /// Hash changed against a healthy release → upgrade.
    Upgrade,
    /// Desired hash matches the deployed release → nothing to do.
    NoOp,
    /// The release is in a failed state → roll back to `to_revision`.
    Rollback { to_revision: u32 },
    /// The custom resource was deleted → uninstall the release.
    Uninstall,
}

/// Decide the reconcile action.
///
/// * `deleted` — the `HelmChart` CR has a deletion timestamp (being removed).
/// * `chart_defaults` / `config_values` — the lower merge layers feeding the
///   desired hash (see [`crate::values::merge_layers`]).
///
/// Decision order matches helm-controller:
/// 1. Deletion wins: uninstall an existing release (no-op if already absent).
/// 2. A failed release is rolled back before anything else.
/// 3. No release → install.
/// 4. Hash differs → upgrade; hash matches → no-op.
#[must_use]
pub fn decide(
    desired: &HelmChart,
    observed: &ReleaseState,
    deleted: bool,
    chart_defaults: Option<Value>,
    config_values: Option<Value>,
) -> Action {
    if deleted {
        return match observed {
            ReleaseState::Absent => Action::NoOp,
            _ => Action::Uninstall,
        };
    }

    match observed {
        // A failed release is rolled back to its last good revision before any
        // upgrade is attempted.
        ReleaseState::Failed {
            last_good_revision, ..
        } => Action::Rollback {
            to_revision: *last_good_revision,
        },
        ReleaseState::Absent => Action::Install,
        ReleaseState::Deployed { applied_hash, .. } => {
            let config =
                config_values.map(|v| crate::chart::HelmChartConfig { values: Some(v) });
            let desired_hash = desired.desired_hash(chart_defaults, config.as_ref());
            if *applied_hash == desired_hash {
                Action::NoOp
            } else {
                Action::Upgrade
            }
        }
    }
}

/// Classification of a failed reconcile, driving the backoff/retry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    /// Transient — retry with backoff (network blip, repo briefly down,
    /// job pod evicted).
    Retryable,
    /// Permanent — the spec is wrong; retrying the same input cannot help
    /// (invalid spec, chart-not-found, bad version). Surface, do not spin.
    Permanent,
}

/// Why a reconcile failed (the categories the controller distinguishes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// Could not reach the repo / network error.
    RepoUnreachable,
    /// The helm job pod was evicted / preempted / timed out.
    JobInterrupted,
    /// Apiserver conflict on status write (optimistic-concurrency).
    Conflict,
    /// The requested chart or version does not exist in the repo.
    ChartNotFound,
    /// The spec itself is invalid.
    InvalidSpec,
    /// Helm template render error (bad values / chart bug).
    RenderError,
}

impl FailureReason {
    /// Classify into the retry policy.
    #[must_use]
    pub const fn classify(self) -> FailureClass {
        match self {
            Self::RepoUnreachable | Self::JobInterrupted | Self::Conflict => {
                FailureClass::Retryable
            }
            Self::ChartNotFound | Self::InvalidSpec | Self::RenderError => FailureClass::Permanent,
        }
    }
}

/// Exponential backoff with a cap, for retryable failures.
///
/// `attempt` is 0-based. Doubles a `base` delay, capped at `cap`. Saturating
/// arithmetic — never panics or overflows.
#[must_use]
pub fn backoff_secs(attempt: u32, base: u64, cap: u64) -> u64 {
    let shift = attempt.min(63);
    let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    base.saturating_mul(factor).min(cap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::{HelmChartSpec, VersionPolicy};
    use std::collections::BTreeMap;

    fn chart() -> HelmChart {
        HelmChart {
            name: "traefik".into(),
            spec: HelmChartSpec {
                chart: "traefik".into(),
                repo: Some("https://helm.traefik.io/traefik".into()),
                version: VersionPolicy::Pinned("1.2.3".into()),
                target_namespace: "kube-system".into(),
                values_content: None,
                set: BTreeMap::new(),
                bootstrap: false,
                job_image: "rancher/klipper-helm:v0.8.0".into(),
            },
            status: crate::chart::HelmChartStatus::default(),
        }
    }

    #[test]
    fn absent_release_installs() {
        let a = decide(&chart(), &ReleaseState::Absent, false, None, None);
        assert_eq!(a, Action::Install);
    }

    #[test]
    fn matching_hash_is_noop() {
        let c = chart();
        let h = c.desired_hash(None, None);
        let obs = ReleaseState::Deployed {
            applied_hash: h,
            revision: 1,
        };
        assert_eq!(decide(&c, &obs, false, None, None), Action::NoOp);
    }

    #[test]
    fn changed_hash_upgrades() {
        let c = chart();
        let obs = ReleaseState::Deployed {
            applied_hash: "stale-hash".into(),
            revision: 1,
        };
        assert_eq!(decide(&c, &obs, false, None, None), Action::Upgrade);
    }

    #[test]
    fn changed_values_flip_the_decision_to_upgrade() {
        let mut c2 = chart();
        let old_hash = c2.desired_hash(None, None);
        // Now the desired spec changes its values: same observed hash is stale.
        c2.spec.set.insert("replicas".into(), Value::Number("3".into()));
        let obs = ReleaseState::Deployed {
            applied_hash: old_hash,
            revision: 1,
        };
        assert_eq!(decide(&c2, &obs, false, None, None), Action::Upgrade);
    }

    #[test]
    fn failed_release_rolls_back_to_last_good() {
        let obs = ReleaseState::Failed {
            applied_hash: "x".into(),
            last_good_revision: 4,
        };
        assert_eq!(
            decide(&chart(), &obs, false, None, None),
            Action::Rollback { to_revision: 4 }
        );
    }

    #[test]
    fn deleted_cr_uninstalls_existing_release() {
        let obs = ReleaseState::Deployed {
            applied_hash: "h".into(),
            revision: 2,
        };
        assert_eq!(decide(&chart(), &obs, true, None, None), Action::Uninstall);
    }

    #[test]
    fn deleted_cr_with_no_release_is_noop() {
        assert_eq!(
            decide(&chart(), &ReleaseState::Absent, true, None, None),
            Action::NoOp
        );
    }

    #[test]
    fn deletion_takes_priority_over_failed_state() {
        let obs = ReleaseState::Failed {
            applied_hash: "x".into(),
            last_good_revision: 1,
        };
        assert_eq!(decide(&chart(), &obs, true, None, None), Action::Uninstall);
    }

    #[test]
    fn config_overlay_change_flips_hash() {
        let c = chart();
        let base = c.desired_hash(None, None);
        let overlay = Value::object().with("extra", Value::String("y".into()));
        let with_overlay = c.desired_hash(
            None,
            Some(&crate::chart::HelmChartConfig {
                values: Some(overlay),
            }),
        );
        assert_ne!(base, with_overlay);
    }

    #[test]
    fn retryable_failures_classified() {
        assert_eq!(
            FailureReason::RepoUnreachable.classify(),
            FailureClass::Retryable
        );
        assert_eq!(
            FailureReason::JobInterrupted.classify(),
            FailureClass::Retryable
        );
        assert_eq!(FailureReason::Conflict.classify(), FailureClass::Retryable);
    }

    #[test]
    fn permanent_failures_classified() {
        assert_eq!(
            FailureReason::ChartNotFound.classify(),
            FailureClass::Permanent
        );
        assert_eq!(
            FailureReason::InvalidSpec.classify(),
            FailureClass::Permanent
        );
        assert_eq!(
            FailureReason::RenderError.classify(),
            FailureClass::Permanent
        );
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff_secs(0, 5, 300), 5);
        assert_eq!(backoff_secs(1, 5, 300), 10);
        assert_eq!(backoff_secs(2, 5, 300), 20);
        assert_eq!(backoff_secs(6, 5, 300), 300); // 5*64=320 -> capped at 300
    }

    #[test]
    fn backoff_never_overflows() {
        assert_eq!(backoff_secs(u32::MAX, 5, 300), 300);
    }
}
