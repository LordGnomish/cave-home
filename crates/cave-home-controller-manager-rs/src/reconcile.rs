// SPDX-License-Identifier: Apache-2.0
//! The reconcile framework: a [`Reconciler`] trait, its [`Outcome`], and the
//! pure decision that maps each outcome onto a [`crate::workqueue::WorkQueue`]
//! operation.
//!
//! Behavioural reimplementation of the controller-runtime "Reconciler returns a
//! `Result{Requeue, RequeueAfter}` or an error, and the loop requeues
//! accordingly" contract. There is no event loop here — only the *decision*: a
//! controller's run loop is `loop { get; reconcile; apply_outcome }`, and
//! [`apply_outcome`] is that final, fully-testable step.

use crate::workqueue::{AddOutcome, WorkQueue};

/// What a [`Reconciler`] asks the loop to do next with the key it processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Reconciled successfully; nothing more to do. Forget the key's backoff.
    Done,
    /// Reconciled, but the controller wants to be re-invoked immediately
    /// (e.g. it made partial progress). Requeued without backoff.
    Requeue,
    /// Re-invoke after at least the given delay (caller's clock unit). Does not
    /// count as a failure; backoff history is preserved but not advanced.
    RequeueAfter(u64),
    /// Reconcile failed. The loop requeues with exponential backoff and may
    /// eventually drop the key.
    Err(String),
}

/// What the loop actually did with the key after applying an [`Outcome`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopAction {
    /// Key finished and was forgotten; not requeued.
    Forgotten,
    /// Key requeued immediately (no backoff).
    Requeued,
    /// Key requeued after an explicit delay.
    RequeuedAfter(u64),
    /// Key requeued by the rate limiter after a failure (with the resulting
    /// failure count and delay).
    RateLimited {
        /// Accumulated failure count for the key.
        failures: u32,
        /// Delay before the key becomes ready again.
        delay: u64,
    },
    /// Key failed too many times and was dropped.
    Dropped {
        /// Failure count at the drop.
        failures: u32,
    },
}

/// The reconcile contract: given an object key, drive observed state toward
/// desired state and report what should happen next.
///
/// `Context` carries whatever the controller needs (a store snapshot, a clock,
/// a recorded set of side effects in tests). The trait is intentionally
/// I/O-free: a reconciler computes a decision over its context; performing the
/// decision (writes to an apiserver) is the caller's job and is deferred.
pub trait Reconciler {
    /// Per-call context (store, clock, effect sink, …).
    type Context;

    /// Reconcile one key. Must not panic.
    fn reconcile(&mut self, key: &str, ctx: &mut Self::Context) -> Outcome;
}

/// Apply a reconcile [`Outcome`] to the queue, mirroring the controller loop's
/// post-reconcile step. `now` is the caller's clock.
///
/// This is the single source of truth for "what does the controller do with
/// each result", kept pure so the policy can be exhaustively tested without a
/// running loop.
pub fn apply_outcome(queue: &mut WorkQueue, key: &str, outcome: &Outcome, now: u64) -> LoopAction {
    match outcome {
        Outcome::Done => {
            queue.forget(key);
            queue.done(key);
            LoopAction::Forgotten
        }
        Outcome::Requeue => {
            queue.forget(key);
            queue.done(key);
            queue.add(key);
            LoopAction::Requeued
        }
        Outcome::RequeueAfter(delay) => {
            queue.forget(key);
            queue.done(key);
            queue.add_after(key, *delay, now);
            LoopAction::RequeuedAfter(*delay)
        }
        Outcome::Err(_) => {
            queue.done(key);
            match queue.add_rate_limited(key, now) {
                AddOutcome::Requeued { failures, delay } => {
                    LoopAction::RateLimited { failures, delay }
                }
                AddOutcome::Dropped { failures } => LoopAction::Dropped { failures },
            }
        }
    }
}

/// Drive a single iteration: pull the next ready key, reconcile it, apply the
/// outcome. Returns `None` if nothing was ready, else the key and the action.
///
/// A real controller wraps this in `loop { … }` with its own clock; exposing
/// one step keeps the loop body testable.
pub fn step<R: Reconciler>(
    reconciler: &mut R,
    queue: &mut WorkQueue,
    ctx: &mut R::Context,
    now: u64,
) -> Option<(String, LoopAction)> {
    let key = queue.get(now)?;
    let outcome = reconciler.reconcile(&key, ctx);
    let action = apply_outcome(queue, &key, &outcome, now);
    Some((key, action))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workqueue::RateLimitConfig;

    fn queue() -> WorkQueue {
        WorkQueue::new(RateLimitConfig {
            base_delay: 10,
            max_delay: 1000,
            max_retries: 2,
        })
    }

    #[test]
    fn done_forgets_and_does_not_requeue() {
        let mut q = queue();
        q.add("a");
        let key = q.get(0).expect("ready");
        let action = apply_outcome(&mut q, &key, &Outcome::Done, 0);
        assert_eq!(action, LoopAction::Forgotten);
        assert!(q.is_empty());
    }

    #[test]
    fn requeue_re_adds_immediately() {
        let mut q = queue();
        q.add("a");
        let key = q.get(0).expect("ready");
        let action = apply_outcome(&mut q, &key, &Outcome::Requeue, 0);
        assert_eq!(action, LoopAction::Requeued);
        assert_eq!(q.get(0).as_deref(), Some("a"));
    }

    #[test]
    fn requeue_after_schedules_delay() {
        let mut q = queue();
        q.add("a");
        let key = q.get(0).expect("ready");
        let action = apply_outcome(&mut q, &key, &Outcome::RequeueAfter(100), 0);
        assert_eq!(action, LoopAction::RequeuedAfter(100));
        assert_eq!(q.get(50), None);
        assert_eq!(q.get(100).as_deref(), Some("a"));
    }

    #[test]
    fn err_rate_limits_then_drops_after_budget() {
        let mut q = queue(); // max_retries = 2
        q.add("a");
        let key = q.get(0).expect("ready");
        let a1 = apply_outcome(&mut q, &key, &Outcome::Err("boom".into()), 0);
        assert_eq!(a1, LoopAction::RateLimited { failures: 1, delay: 10 });

        let key = q.get(1_000_000).expect("ready after backoff");
        let a2 = apply_outcome(&mut q, &key, &Outcome::Err("boom".into()), 1_000_000);
        assert_eq!(a2, LoopAction::RateLimited { failures: 2, delay: 20 });

        let key = q.get(2_000_000).expect("ready after backoff");
        let a3 = apply_outcome(&mut q, &key, &Outcome::Err("boom".into()), 2_000_000);
        assert_eq!(a3, LoopAction::Dropped { failures: 3 });
        assert!(q.is_empty());
    }

    #[test]
    fn success_after_failure_resets_backoff() {
        let mut q = queue();
        q.add("a");
        let key = q.get(0).expect("ready");
        let _ = apply_outcome(&mut q, &key, &Outcome::Err("e".into()), 0);
        assert_eq!(q.retries("a"), 1);
        let key = q.get(1_000_000).expect("ready");
        let _ = apply_outcome(&mut q, &key, &Outcome::Done, 1_000_000);
        assert_eq!(q.retries("a"), 0, "Done forgets backoff");
    }

    // A toy reconciler exercising `step` end-to-end.
    struct CountTo {
        target: u32,
        seen: u32,
    }
    impl Reconciler for CountTo {
        type Context = Vec<String>;
        fn reconcile(&mut self, key: &str, ctx: &mut Vec<String>) -> Outcome {
            ctx.push(key.to_owned());
            self.seen += 1;
            if self.seen >= self.target {
                Outcome::Done
            } else {
                Outcome::Requeue
            }
        }
    }

    #[test]
    fn step_drives_reconciler_until_done() {
        let mut q = queue();
        let mut r = CountTo { target: 3, seen: 0 };
        let mut ctx = Vec::new();
        q.add("k");
        let mut actions = Vec::new();
        let mut now = 0;
        while let Some((_key, action)) = step(&mut r, &mut q, &mut ctx, now) {
            actions.push(action);
            now += 1;
        }
        assert_eq!(ctx, vec!["k", "k", "k"]);
        assert_eq!(
            actions,
            vec![LoopAction::Requeued, LoopAction::Requeued, LoopAction::Forgotten]
        );
    }

    #[test]
    fn step_returns_none_on_empty_queue() {
        let mut q = queue();
        let mut r = CountTo { target: 1, seen: 0 };
        let mut ctx = Vec::new();
        assert!(step(&mut r, &mut q, &mut ctx, 0).is_none());
    }
}
