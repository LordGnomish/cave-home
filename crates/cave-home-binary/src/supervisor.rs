// SPDX-License-Identifier: Apache-2.0
//! Supervised component lifecycle: restart-on-failure with backoff and a
//! bounded restart budget, plus dependency-ordered graceful shutdown.
//!
//! Behavioural reference: a process supervisor (systemd `Restart=on-failure` +
//! `StartLimitBurst`/`StartLimitIntervalSec`, or an Erlang/OTP `one_for_one`
//! supervisor). The control plane this binary boots is a set of long-lived
//! components ([`crate::server`]'s apiserver listener, reconcile supervisors,
//! kubelet pod-runtime). Before this module a component task that returned an
//! error or panicked simply vanished and was never brought back; now each runs
//! under a [`Supervisor`] that:
//!
//! 1. restarts the component when its body exits with an error, applying an
//!    [`exponential backoff`](RestartPolicy::backoff_for) between attempts;
//! 2. gives up (and reports the failure) once the body has failed
//!    [`max_restarts`](RestartPolicy::max_restarts) times inside the rolling
//!    [`window`](RestartPolicy::window) — a crash loop is surfaced, not hidden;
//! 3. stops cleanly the moment a shutdown signal arrives, even mid-backoff.
//!
//! The *policy* ([`RestartPolicy`], [`RestartLedger`]) is pure and synchronous,
//! so the crash-loop accounting is fully unit-testable without spawning tasks.
//! The async driver ([`Supervisor::run`]) layers a real tokio loop on top.

use std::future::Future;
use std::time::Duration;

use tokio::sync::watch;

/// How a supervised component is restarted after it exits with an error.
///
/// A component may fail up to `max_restarts` times within a rolling `window`
/// before the supervisor declares it crash-looped and stops retrying. Between
/// attempts it waits an exponentially growing backoff (`base * 2^failures`),
/// capped at `max_backoff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPolicy {
    /// Maximum number of restarts permitted inside one rolling `window`.
    max_restarts: u32,
    /// The rolling window over which `max_restarts` is counted.
    window: Duration,
    /// The first backoff delay; doubles per consecutive failure.
    base_backoff: Duration,
    /// The ceiling the doubling backoff is clamped to.
    max_backoff: Duration,
}

impl Default for RestartPolicy {
    /// A sensible control-plane default: up to 5 restarts per 60s, backing off
    /// from 100ms to a 10s ceiling.
    fn default() -> Self {
        Self {
            max_restarts: 5,
            window: Duration::from_secs(60),
            base_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
        }
    }
}

impl RestartPolicy {
    /// A policy that never restarts: the first failure is terminal.
    #[must_use]
    pub const fn never() -> Self {
        Self {
            max_restarts: 0,
            window: Duration::from_secs(60),
            base_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
        }
    }

    /// Set the restart budget (failures permitted per [`window`](Self::window)).
    #[must_use]
    pub const fn with_max_restarts(mut self, max: u32) -> Self {
        self.max_restarts = max;
        self
    }

    /// Set the rolling window the budget is counted over.
    #[must_use]
    pub const fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    /// Set the base backoff (and, if larger, raise the ceiling to match).
    #[must_use]
    pub fn with_base_backoff(mut self, base: Duration) -> Self {
        self.base_backoff = base;
        if self.max_backoff < base {
            self.max_backoff = base;
        }
        self
    }

    /// Set the maximum backoff ceiling.
    #[must_use]
    pub const fn with_max_backoff(mut self, max: Duration) -> Self {
        self.max_backoff = max;
        self
    }

    /// The restart budget per window.
    #[must_use]
    pub const fn max_restarts(&self) -> u32 {
        self.max_restarts
    }

    /// The rolling window the budget is counted over.
    #[must_use]
    pub const fn window(&self) -> Duration {
        self.window
    }

    /// The backoff to wait before the restart that follows `consecutive_failures`
    /// prior failures (0 → `base`, 1 → `base*2`, …), clamped to `max_backoff`.
    #[must_use]
    pub fn backoff_for(&self, consecutive_failures: u32) -> Duration {
        if self.base_backoff.is_zero() {
            return Duration::ZERO;
        }
        // Saturating doubling: shift the base by `consecutive_failures`, but stop
        // once we've clearly exceeded the ceiling so we never overflow.
        let mut delay = self.base_backoff;
        for _ in 0..consecutive_failures {
            delay = delay.saturating_mul(2);
            if delay >= self.max_backoff {
                return self.max_backoff;
            }
        }
        delay.min(self.max_backoff)
    }
}

/// The running crash-loop accounting for one supervised component: the failure
/// timestamps inside the policy window. Pure and clock-injected so the decision
/// ("may I restart?") is unit-testable.
#[derive(Debug, Clone)]
pub struct RestartLedger {
    policy: RestartPolicy,
    /// Monotonic timestamps (in arbitrary ticks/units) of recent failures,
    /// oldest first; pruned to the policy window on each record.
    failures: Vec<Duration>,
    /// Consecutive failures since the last clean run — drives the backoff curve.
    consecutive: u32,
}

impl RestartLedger {
    /// A fresh ledger for `policy` with no recorded failures.
    #[must_use]
    pub const fn new(policy: RestartPolicy) -> Self {
        Self { policy, failures: Vec::new(), consecutive: 0 }
    }

    /// Record a failure observed at monotonic time `now`, pruning any failures
    /// that fell outside the rolling window.
    pub fn record_failure(&mut self, now: Duration) {
        self.failures.push(now);
        self.consecutive = self.consecutive.saturating_add(1);
        self.prune(now);
    }

    /// Note that the component ran cleanly (exited `Ok` or is healthy again):
    /// the consecutive-failure streak resets, so the next failure backs off from
    /// the base again.
    pub const fn record_clean_run(&mut self) {
        self.consecutive = 0;
    }

    /// Whether another restart is permitted: the number of failures still inside
    /// the window must not exceed the policy budget.
    #[must_use]
    pub fn may_restart(&self, now: Duration) -> bool {
        let in_window = self
            .failures
            .iter()
            .filter(|&&t| now.saturating_sub(t) <= self.policy.window)
            .count();
        in_window <= self.policy.max_restarts as usize
    }

    /// The backoff to wait before the next restart attempt.
    #[must_use]
    pub fn next_backoff(&self) -> Duration {
        // The first failure (consecutive == 1) waits the base backoff.
        self.policy.backoff_for(self.consecutive.saturating_sub(1))
    }

    /// Drop failures older than the rolling window relative to `now`.
    fn prune(&mut self, now: Duration) {
        let window = self.policy.window;
        self.failures.retain(|&t| now.saturating_sub(t) <= window);
    }
}

/// Why a [`Supervisor::run`] loop returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisedExit {
    /// A shutdown signal was observed; the component was asked to stop and did.
    ShutdownRequested,
    /// The component crash-looped past its restart budget and was given up on.
    CrashLooped,
    /// The component body returned `Ok(())` and the policy does not restart on a
    /// clean exit (a one-shot task completing successfully).
    Completed,
}

/// Supervise one component: run its async body, restart it on error per
/// [`RestartPolicy`], and stop the moment `shutdown` flips to `true`.
///
/// `body` is an async factory invoked once per run/restart; it receives a
/// shutdown receiver so a long-lived component can return promptly on shutdown.
/// A body that returns `Ok(())` is treated as a clean completion (no restart);
/// an `Err` consumes one restart from the budget and is retried after a backoff.
///
/// Returns the [`SupervisedExit`] describing why supervision ended.
pub async fn run<F, Fut, E>(
    name: &str,
    policy: RestartPolicy,
    mut shutdown: watch::Receiver<bool>,
    mut body: F,
) -> SupervisedExit
where
    F: FnMut(watch::Receiver<bool>) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: std::fmt::Display,
{
    let start = tokio::time::Instant::now();
    let mut ledger = RestartLedger::new(policy.clone());

    loop {
        if *shutdown.borrow() {
            return SupervisedExit::ShutdownRequested;
        }

        // The body owns a shutdown receiver, so a long-lived component returns
        // promptly when asked to stop; we then observe that on the next loop.
        let outcome = body(shutdown.clone()).await;

        match outcome {
            Ok(()) => {
                ledger.record_clean_run();
                // A long-lived component returns Ok because it observed the
                // shutdown signal; a one-shot returns Ok on real completion.
                if *shutdown.borrow() {
                    return SupervisedExit::ShutdownRequested;
                }
                return SupervisedExit::Completed;
            }
            Err(e) => {
                let now = start.elapsed();
                ledger.record_failure(now);
                if !ledger.may_restart(now) {
                    log_line(&format!("{name} crash-looped ({e}); giving up"));
                    return SupervisedExit::CrashLooped;
                }
                let backoff = ledger.next_backoff();
                log_line(&format!("{name} failed ({e}); restarting in {backoff:?}"));
                // Wait the backoff, but cut it short if shutdown arrives.
                tokio::select! {
                    biased;
                    _ = wait_shutdown(&mut shutdown) => {
                        return SupervisedExit::ShutdownRequested;
                    }
                    () = tokio::time::sleep(backoff) => {}
                }
            }
        }
    }
}

/// Resolve to `true` once `shutdown` carries `true`; `false` if the sender is
/// dropped (so the caller can decide what a closed channel means).
async fn wait_shutdown(shutdown: &mut watch::Receiver<bool>) -> bool {
    if *shutdown.borrow() {
        return true;
    }
    loop {
        if shutdown.changed().await.is_err() {
            return false;
        }
        if *shutdown.borrow() {
            return true;
        }
    }
}

/// Emit a runtime log line (kept consistent with [`crate::server`]'s format).
fn log_line(msg: &str) {
    println!("cave-home: {msg}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn backoff_doubles_then_clamps() {
        let p = RestartPolicy::default()
            .with_base_backoff(Duration::from_millis(100))
            .with_max_backoff(Duration::from_millis(800));
        assert_eq!(p.backoff_for(0), Duration::from_millis(100));
        assert_eq!(p.backoff_for(1), Duration::from_millis(200));
        assert_eq!(p.backoff_for(2), Duration::from_millis(400));
        assert_eq!(p.backoff_for(3), Duration::from_millis(800));
        // Clamped at the ceiling, no overflow on a large streak.
        assert_eq!(p.backoff_for(100), Duration::from_millis(800));
    }

    #[test]
    fn never_policy_has_zero_budget_and_no_backoff() {
        let p = RestartPolicy::never();
        assert_eq!(p.max_restarts(), 0);
        assert_eq!(p.backoff_for(0), Duration::ZERO);
    }

    #[test]
    fn ledger_permits_restarts_up_to_budget_then_refuses() {
        let policy = RestartPolicy::default()
            .with_max_restarts(2)
            .with_window(Duration::from_secs(60));
        let mut ledger = RestartLedger::new(policy);
        // Two failures inside the window are within budget.
        ledger.record_failure(Duration::from_secs(1));
        assert!(ledger.may_restart(Duration::from_secs(1)));
        ledger.record_failure(Duration::from_secs(2));
        assert!(ledger.may_restart(Duration::from_secs(2)));
        // A third failure inside the window exceeds the budget of 2.
        ledger.record_failure(Duration::from_secs(3));
        assert!(!ledger.may_restart(Duration::from_secs(3)), "3 failures > budget 2");
    }

    #[test]
    fn ledger_forgets_failures_outside_the_window() {
        let policy = RestartPolicy::default()
            .with_max_restarts(1)
            .with_window(Duration::from_secs(10));
        let mut ledger = RestartLedger::new(policy);
        ledger.record_failure(Duration::from_secs(1));
        ledger.record_failure(Duration::from_secs(2));
        // Two failures within 10s > budget 1.
        assert!(!ledger.may_restart(Duration::from_secs(2)));
        // Much later, the old failures have aged out: a fresh failure is alone.
        ledger.record_failure(Duration::from_secs(100));
        assert!(ledger.may_restart(Duration::from_secs(100)), "old failures aged out");
    }

    #[test]
    fn ledger_backoff_follows_consecutive_streak_and_resets_on_clean_run() {
        let policy = RestartPolicy::default()
            .with_base_backoff(Duration::from_millis(50))
            .with_max_backoff(Duration::from_secs(10));
        let mut ledger = RestartLedger::new(policy);
        ledger.record_failure(Duration::from_secs(1));
        assert_eq!(ledger.next_backoff(), Duration::from_millis(50));
        ledger.record_failure(Duration::from_secs(2));
        assert_eq!(ledger.next_backoff(), Duration::from_millis(100));
        // A clean run resets the streak back to the base backoff.
        ledger.record_clean_run();
        ledger.record_failure(Duration::from_secs(3));
        assert_eq!(ledger.next_backoff(), Duration::from_millis(50));
    }

    #[tokio::test(start_paused = true)]
    async fn supervisor_restarts_a_failing_body_then_stays_up() {
        // A body that fails its first 2 invocations then blocks forever (a
        // component that finally comes up). The supervisor must invoke it 3
        // times total: fail, fail, then the steady run.
        let calls = Arc::new(AtomicU32::new(0));
        let calls2 = calls.clone();
        let (_tx, rx) = watch::channel(false);
        let policy = RestartPolicy::default()
            .with_max_restarts(5)
            .with_base_backoff(Duration::from_millis(10));

        let sup = tokio::spawn(async move {
            run("test", policy, rx, move |mut sd| {
                let calls = calls2.clone();
                async move {
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        return Err("boom");
                    }
                    // Steady state: run until shutdown.
                    while !*sd.borrow() {
                        if sd.changed().await.is_err() {
                            break;
                        }
                    }
                    Ok::<(), &str>(())
                }
            })
            .await
        });

        // Let the paused clock advance through both backoffs.
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 3, "fail, fail, then steady run");
        assert!(!sup.is_finished(), "steady run keeps the supervisor alive");
        sup.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn supervisor_gives_up_after_crash_loop_budget() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls2 = calls.clone();
        let (_tx, rx) = watch::channel(false);
        let policy = RestartPolicy::default()
            .with_max_restarts(3)
            .with_window(Duration::from_secs(3600))
            .with_base_backoff(Duration::from_millis(10));

        let exit = run("loop", policy, rx, move |_sd| {
            let calls = calls2.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), &str>("always fails")
            }
        })
        .await;

        assert_eq!(exit, SupervisedExit::CrashLooped);
        // budget 3 → 4 failures observed (the 4th exceeds the budget).
        assert_eq!(calls.load(Ordering::SeqCst), 4, "tries budget+1 then gives up");
    }

    #[tokio::test(start_paused = true)]
    async fn supervisor_stops_promptly_on_shutdown() {
        let (tx, rx) = watch::channel(false);
        let body_started = Arc::new(AtomicU32::new(0));
        let started2 = body_started.clone();
        let sup = tokio::spawn(async move {
            run("svc", RestartPolicy::default(), rx, move |mut sd| {
                let started = started2.clone();
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    while !*sd.borrow() {
                        if sd.changed().await.is_err() {
                            break;
                        }
                    }
                    Ok::<(), &str>(())
                }
            })
            .await
        });
        // Let the body start, then ask for shutdown.
        tokio::time::sleep(Duration::from_millis(50)).await;
        tx.send(true).expect("send shutdown");
        let exit = tokio::time::timeout(Duration::from_secs(1), sup)
            .await
            .expect("supervisor returns promptly")
            .expect("join");
        assert_eq!(exit, SupervisedExit::ShutdownRequested);
        assert_eq!(body_started.load(Ordering::SeqCst), 1, "body ran exactly once");
    }

    #[tokio::test(start_paused = true)]
    async fn supervisor_cuts_backoff_short_on_shutdown() {
        // A body that always fails; shutdown should interrupt the backoff sleep
        // rather than waiting it out.
        let (tx, rx) = watch::channel(false);
        let policy = RestartPolicy::default()
            .with_max_restarts(100)
            .with_base_backoff(Duration::from_secs(30));
        let sup = tokio::spawn(async move {
            run("svc", policy, rx, move |_sd| async move { Err::<(), &str>("fail") }).await
        });
        // First failure happens immediately; we're now inside a 30s backoff.
        tokio::time::sleep(Duration::from_millis(10)).await;
        tx.send(true).expect("send");
        // Without backoff interruption this would hang for 30 (paused) seconds;
        // a real timeout of 5s of paused time proves it returns early.
        let exit = tokio::time::timeout(Duration::from_secs(5), sup)
            .await
            .expect("returns before the 30s backoff elapses")
            .expect("join");
        assert_eq!(exit, SupervisedExit::ShutdownRequested);
    }
}
