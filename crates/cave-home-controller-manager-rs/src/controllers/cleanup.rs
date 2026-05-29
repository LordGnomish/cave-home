// SPDX-License-Identifier: Apache-2.0
//! Cleanup controllers — TTL-after-finished and namespace-deletion finalizer
//! sweep.
//!
//! Two small, pure decision functions:
//!
//! * **TTL-after-finished** (`pkg/controller/ttlafterfinished` contract): a Job
//!   that finished (completed or failed) at time `t` with
//!   `ttlSecondsAfterFinished = ttl` should be deleted once `now >= t + ttl`.
//!   Until then, the controller reports how long to wait so it can requeue.
//!
//! * **Namespace deletion finalizer sweep**
//!   (`pkg/controller/namespace` contract): a namespace marked terminating is
//!   finalized — its `kubernetes` finalizer removed — only once it holds no
//!   remaining content. While content remains, the sweep reports the content
//!   to delete first.
//!
//! `std` only; `now` is caller-supplied epoch seconds.

use crate::types::ObjectMeta;

/// The standard namespace finalizer that gates namespace deletion.
pub const NAMESPACE_FINALIZER: &str = "kubernetes";

/// A finished job's TTL inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinishedJob {
    /// Epoch-seconds the job finished (completed or failed).
    pub finished_at: i64,
    /// `ttlSecondsAfterFinished`. `None` means the job is never auto-cleaned.
    pub ttl: Option<i64>,
}

/// What the TTL controller decides for a finished job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtlDecision {
    /// No TTL configured; leave the job alone.
    NoTtl,
    /// TTL has elapsed; delete the job now.
    Delete,
    /// TTL not yet elapsed; requeue after this many seconds.
    RequeueAfter(i64),
}

/// Decide the TTL-after-finished action for a finished job at `now`.
///
/// A non-positive TTL means "delete immediately" (upstream treats `0` as
/// eligible at the finish instant).
#[must_use]
pub fn ttl_decision(job: &FinishedJob, now: i64) -> TtlDecision {
    let Some(ttl) = job.ttl else {
        return TtlDecision::NoTtl;
    };
    let expire_at = job.finished_at.saturating_add(ttl.max(0));
    if now >= expire_at {
        TtlDecision::Delete
    } else {
        TtlDecision::RequeueAfter(expire_at - now)
    }
}

/// The outcome of a namespace finalizer sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamespaceSweep {
    /// Namespace is not terminating; nothing to do.
    NotTerminating,
    /// Still holds content; these object keys must be deleted before the
    /// finalizer can be removed.
    AwaitingContent {
        /// Keys of objects still present in the namespace.
        remaining: Vec<String>,
    },
    /// No content remains; remove the `kubernetes` finalizer (which lets the
    /// apiserver delete the namespace object).
    RemoveFinalizer,
    /// Already finalized (terminating, no finalizer left); nothing to do.
    AlreadyFinalized,
}

/// Decide the namespace-deletion sweep action.
///
/// `namespace_meta` is the namespace object; `contents` is the set of object
/// keys still living in the namespace (the caller enumerates them from the
/// store). Mirrors the upstream rule: drive content to empty, then drop the
/// finalizer.
#[must_use]
pub fn namespace_sweep(namespace_meta: &ObjectMeta, contents: &[String]) -> NamespaceSweep {
    if !namespace_meta.is_terminating() {
        return NamespaceSweep::NotTerminating;
    }
    if !contents.is_empty() {
        let mut remaining: Vec<String> = contents.to_vec();
        remaining.sort();
        remaining.dedup();
        return NamespaceSweep::AwaitingContent { remaining };
    }
    if namespace_meta
        .finalizers
        .iter()
        .any(|f| f == NAMESPACE_FINALIZER)
    {
        NamespaceSweep::RemoveFinalizer
    } else {
        NamespaceSweep::AlreadyFinalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_ttl_leaves_job_alone() {
        let j = FinishedJob { finished_at: 100, ttl: None };
        assert_eq!(ttl_decision(&j, 10_000), TtlDecision::NoTtl);
    }

    #[test]
    fn ttl_not_elapsed_requeues_with_remaining() {
        let j = FinishedJob { finished_at: 100, ttl: Some(60) };
        assert_eq!(ttl_decision(&j, 130), TtlDecision::RequeueAfter(30));
    }

    #[test]
    fn ttl_elapsed_deletes() {
        let j = FinishedJob { finished_at: 100, ttl: Some(60) };
        assert_eq!(ttl_decision(&j, 160), TtlDecision::Delete);
        assert_eq!(ttl_decision(&j, 200), TtlDecision::Delete);
    }

    #[test]
    fn ttl_exactly_at_boundary_deletes() {
        let j = FinishedJob { finished_at: 100, ttl: Some(60) };
        assert_eq!(ttl_decision(&j, 160), TtlDecision::Delete);
    }

    #[test]
    fn zero_ttl_deletes_at_finish_instant() {
        let j = FinishedJob { finished_at: 100, ttl: Some(0) };
        assert_eq!(ttl_decision(&j, 100), TtlDecision::Delete);
    }

    #[test]
    fn namespace_not_terminating_is_noop() {
        let ns = ObjectMeta::new("dev", "", "u").with_finalizer(NAMESPACE_FINALIZER);
        assert_eq!(namespace_sweep(&ns, &[]), NamespaceSweep::NotTerminating);
    }

    #[test]
    fn terminating_namespace_with_content_awaits() {
        let mut ns = ObjectMeta::new("dev", "", "u").with_finalizer(NAMESPACE_FINALIZER);
        ns.deletion_timestamp = Some(500);
        let contents = vec!["dev/item-b".to_owned(), "dev/item-a".to_owned()];
        match namespace_sweep(&ns, &contents) {
            NamespaceSweep::AwaitingContent { remaining } => {
                assert_eq!(remaining, vec!["dev/item-a", "dev/item-b"], "sorted");
            }
            other => panic!("expected AwaitingContent, got {other:?}"),
        }
    }

    #[test]
    fn terminating_empty_namespace_removes_finalizer() {
        let mut ns = ObjectMeta::new("dev", "", "u").with_finalizer(NAMESPACE_FINALIZER);
        ns.deletion_timestamp = Some(500);
        assert_eq!(namespace_sweep(&ns, &[]), NamespaceSweep::RemoveFinalizer);
    }

    #[test]
    fn terminating_empty_without_finalizer_is_already_finalized() {
        let mut ns = ObjectMeta::new("dev", "", "u");
        ns.deletion_timestamp = Some(500);
        assert_eq!(namespace_sweep(&ns, &[]), NamespaceSweep::AlreadyFinalized);
    }

    #[test]
    fn namespace_sweep_dedups_remaining_content() {
        let mut ns = ObjectMeta::new("dev", "", "u").with_finalizer(NAMESPACE_FINALIZER);
        ns.deletion_timestamp = Some(1);
        let contents = vec!["dev/x".to_owned(), "dev/x".to_owned()];
        match namespace_sweep(&ns, &contents) {
            NamespaceSweep::AwaitingContent { remaining } => assert_eq!(remaining, vec!["dev/x"]),
            other => panic!("expected AwaitingContent, got {other:?}"),
        }
    }
}
