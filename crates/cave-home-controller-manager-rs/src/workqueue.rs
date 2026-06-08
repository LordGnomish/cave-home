// SPDX-License-Identifier: Apache-2.0
//! A rate-limited, delaying work queue — the heart of every controller loop.
//!
//! Behavioural reimplementation of the documented contract of
//! `k8s.io/client-go/util/workqueue` (the `Type`, `DelayingInterface` and
//! `RateLimitingInterface` shapes), built on `std` only. There is **no**
//! background goroutine and **no** clock dependency: time is a monotonic
//! `now` (epoch-millis-ish, any consistent unit) **supplied by the caller** on
//! every operation. A controller's run loop is expected to pass its own clock.
//!
//! Implemented behaviours (each tested):
//! * **dedup** — a key already queued (or in-flight and re-added) is collapsed
//!   into a single entry; it is never processed concurrently with itself.
//! * **delaying add (`add_after`)** — a key can be scheduled to become ready at
//!   `now + delay`; [`WorkQueue::ready`] promotes due items.
//! * **per-key exponential backoff** — [`WorkQueue::add_rate_limited`] schedules
//!   a key after `base * 2^(failures-1)`, capped at `max`, where `failures` is
//!   the per-key retry count.
//! * **max-retries → drop** — once a key exceeds the retry budget it is dropped
//!   instead of requeued, and the caller is told so it can stop tracking it.
//!
//! The queue is single-threaded and owns no locks; concurrency is the caller's
//! concern (a real controller wraps it in a mutex). This keeps the *decision*
//! logic — what gets processed when — pure and exhaustively testable.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// Knobs for the per-key exponential-backoff rate limiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitConfig {
    /// Delay applied on the first failure (same unit as the caller's clock).
    pub base_delay: u64,
    /// Upper bound on the computed backoff delay.
    pub max_delay: u64,
    /// Retries permitted before a key is dropped. `add_rate_limited` returns
    /// [`AddOutcome::Dropped`] once the per-key failure count exceeds this.
    pub max_retries: u32,
}

impl Default for RateLimitConfig {
    /// Mirrors client-go's `DefaultControllerRateLimiter` ordering of magnitude:
    /// 5 ms base, 1000 s cap, doubling each failure. `max_retries` defaults to
    /// 15, a common controller budget.
    fn default() -> Self {
        Self {
            base_delay: 5,
            max_delay: 1_000_000,
            max_retries: 15,
        }
    }
}

impl RateLimitConfig {
    /// Backoff delay for the `failures`-th consecutive failure (1-based).
    ///
    /// `base * 2^(failures-1)`, saturating and clamped to `max_delay`. Returns
    /// `base_delay` for `failures == 0` (treated as the first attempt).
    #[must_use]
    pub fn backoff_for(&self, failures: u32) -> u64 {
        if failures <= 1 {
            return self.base_delay.min(self.max_delay);
        }
        // base << (failures - 1), guarding the shift and the multiply.
        let shift = failures - 1;
        if shift >= 64 {
            return self.max_delay;
        }
        let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        self.base_delay.saturating_mul(factor).min(self.max_delay)
    }
}

/// Result of a rate-limited add: requeued after a delay, or dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    /// The key was scheduled to become ready after the given delay.
    Requeued {
        /// Number of accumulated failures for this key.
        failures: u32,
        /// Delay applied before the key becomes ready again.
        delay: u64,
    },
    /// The key exceeded the retry budget and was dropped; the caller should
    /// forget it.
    Dropped {
        /// Failure count at the point of dropping.
        failures: u32,
    },
}

/// A rate-limited, delaying work queue keyed by `String`.
#[derive(Debug)]
pub struct WorkQueue {
    cfg: RateLimitConfig,
    /// Keys ready to be processed, in FIFO order.
    ready: VecDeque<String>,
    /// Set membership of `ready` + `delayed`, for O(1) dedup.
    queued: HashSet<String>,
    /// Keys currently handed out via `get`, not yet `done`.
    processing: HashSet<String>,
    /// Keys re-added while processing; re-enqueued on `done` (dirty set).
    dirty_while_processing: HashSet<String>,
    /// Keys waiting for their ready-time: `ready_at` -> keys.
    delayed: BTreeMap<u64, Vec<String>>,
    /// Per-key accumulated failure count, for backoff and the retry budget.
    failures: HashMap<String, u32>,
}

impl WorkQueue {
    /// A queue with the given rate-limit configuration.
    #[must_use]
    pub fn new(cfg: RateLimitConfig) -> Self {
        Self {
            cfg,
            ready: VecDeque::new(),
            queued: HashSet::new(),
            processing: HashSet::new(),
            dirty_while_processing: HashSet::new(),
            delayed: BTreeMap::new(),
            failures: HashMap::new(),
        }
    }

    /// A queue with the default rate-limit configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(RateLimitConfig::default())
    }

    /// Add `key` to the ready queue immediately, deduplicating.
    ///
    /// If the key is already ready or delayed, this is a no-op. If the key is
    /// currently being processed, it is marked dirty and will be re-enqueued
    /// when [`WorkQueue::done`] is called — never processed twice at once.
    pub fn add(&mut self, key: &str) {
        if self.processing.contains(key) {
            self.dirty_while_processing.insert(key.to_owned());
            return;
        }
        if self.ready_contains(key) {
            return; // already ready
        }
        // An immediate add supersedes any pending delayed schedule for the key.
        if self.delayed_ready_at(key).is_some() {
            self.remove_delayed(key);
        }
        self.queued.insert(key.to_owned());
        self.ready.push_back(key.to_owned());
    }

    /// Schedule `key` to become ready at `now + delay` (delaying-queue add).
    ///
    /// A `delay` of 0 is equivalent to [`WorkQueue::add`]. If the key is already
    /// ready, the earlier (immediate) placement wins and this is a no-op. If the
    /// key is already delayed, the **earlier** ready-time wins.
    pub fn add_after(&mut self, key: &str, delay: u64, now: u64) {
        if delay == 0 {
            self.add(key);
            return;
        }
        if self.processing.contains(key) {
            // Will be reconsidered on done(); record the intent as dirty.
            self.dirty_while_processing.insert(key.to_owned());
            return;
        }
        if self.ready_contains(key) {
            return; // already ready sooner than any delay.
        }
        let ready_at = now.saturating_add(delay);
        // Earliest ready-time wins: drop any later existing schedule.
        if let Some(existing) = self.delayed_ready_at(key) {
            if existing <= ready_at {
                return;
            }
            self.remove_delayed(key);
        }
        self.queued.insert(key.to_owned());
        self.delayed.entry(ready_at).or_default().push(key.to_owned());
    }

    /// Add `key` with exponential backoff derived from its failure history.
    ///
    /// Increments the key's failure count, then either schedules it after the
    /// computed backoff or, if the retry budget is exhausted, drops it (and
    /// forgets its history). Returns the [`AddOutcome`] so the caller can react.
    pub fn add_rate_limited(&mut self, key: &str, now: u64) -> AddOutcome {
        let failures = self.failures.entry(key.to_owned()).or_insert(0);
        *failures += 1;
        let n = *failures;
        if n > self.cfg.max_retries {
            self.failures.remove(key);
            return AddOutcome::Dropped { failures: n };
        }
        let delay = self.cfg.backoff_for(n);
        self.add_after(key, delay, now);
        AddOutcome::Requeued { failures: n, delay }
    }

    /// Promote every delayed key whose ready-time is `<= now` into the ready
    /// queue, then return the next ready key for processing (FIFO), if any.
    ///
    /// The returned key is marked in-flight; the caller must call
    /// [`WorkQueue::done`] when finished so a concurrent re-add can be honoured.
    pub fn get(&mut self, now: u64) -> Option<String> {
        self.flush_due(now);
        let key = self.ready.pop_front()?;
        self.queued.remove(&key);
        self.processing.insert(key.clone());
        Some(key)
    }

    /// Mark a previously-`get`-returned key as finished.
    ///
    /// If it was re-added while in flight (dirty), it is re-enqueued now.
    pub fn done(&mut self, key: &str) {
        self.processing.remove(key);
        if self.dirty_while_processing.remove(key) {
            self.add(key);
        }
    }

    /// Clear a key's failure history (client-go `Forget`). Call after a
    /// successful reconcile so the next failure starts backoff from the base.
    pub fn forget(&mut self, key: &str) {
        self.failures.remove(key);
    }

    /// Current per-key failure count (0 if unknown).
    #[must_use]
    pub fn retries(&self, key: &str) -> u32 {
        self.failures.get(key).copied().unwrap_or(0)
    }

    /// Number of keys ready to process right now (excludes delayed/in-flight).
    #[must_use]
    pub fn ready_len(&self) -> usize {
        self.ready.len()
    }

    /// Number of keys waiting on a delay.
    #[must_use]
    pub fn delayed_len(&self) -> usize {
        self.delayed.values().map(Vec::len).sum()
    }

    /// Total tracked keys (ready + delayed), excluding in-flight.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ready_len() + self.delayed_len()
    }

    /// `true` if nothing is ready or delayed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // ---- internals --------------------------------------------------------

    fn flush_due(&mut self, now: u64) {
        // BTreeMap is ordered: pop every bucket with ready_at <= now.
        let due: Vec<u64> = self
            .delayed
            .range(..=now)
            .map(|(k, _)| *k)
            .collect();
        for at in due {
            if let Some(keys) = self.delayed.remove(&at) {
                for key in keys {
                    // queued flag already set; just move into ready FIFO.
                    if self.queued.contains(&key) && !self.ready_contains(&key) {
                        self.ready.push_back(key);
                    }
                }
            }
        }
    }

    fn ready_contains(&self, key: &str) -> bool {
        self.ready.iter().any(|k| k == key)
    }

    fn delayed_ready_at(&self, key: &str) -> Option<u64> {
        self.delayed
            .iter()
            .find(|(_, ks)| ks.iter().any(|k| k == key))
            .map(|(at, _)| *at)
    }

    fn remove_delayed(&mut self, key: &str) {
        let mut empty = Vec::new();
        for (at, ks) in &mut self.delayed {
            ks.retain(|k| k != key);
            if ks.is_empty() {
                empty.push(*at);
            }
        }
        for at in empty {
            self.delayed.remove(&at);
        }
        self.queued.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RateLimitConfig {
        RateLimitConfig {
            base_delay: 10,
            max_delay: 1000,
            max_retries: 3,
        }
    }

    #[test]
    fn add_then_get_is_fifo() {
        let mut q = WorkQueue::new(cfg());
        q.add("a");
        q.add("b");
        q.add("c");
        assert_eq!(q.get(0).as_deref(), Some("a"));
        assert_eq!(q.get(0).as_deref(), Some("b"));
        assert_eq!(q.get(0).as_deref(), Some("c"));
        assert_eq!(q.get(0), None);
    }

    #[test]
    fn add_dedups_same_key() {
        let mut q = WorkQueue::new(cfg());
        q.add("a");
        q.add("a");
        q.add("a");
        assert_eq!(q.ready_len(), 1);
        assert_eq!(q.get(0).as_deref(), Some("a"));
        assert_eq!(q.get(0), None);
    }

    #[test]
    fn readd_while_processing_requeues_once_on_done() {
        let mut q = WorkQueue::new(cfg());
        q.add("a");
        let item = q.get(0).expect("ready item");
        assert_eq!(item, "a");
        // Re-added (twice) while in flight: must not be visible yet.
        q.add("a");
        q.add("a");
        assert_eq!(q.get(0), None, "in-flight key must not be handed out again");
        q.done("a");
        // Now exactly one re-enqueue is visible.
        assert_eq!(q.get(0).as_deref(), Some("a"));
        q.done("a");
        assert_eq!(q.get(0), None);
    }

    #[test]
    fn done_without_readd_does_not_requeue() {
        let mut q = WorkQueue::new(cfg());
        q.add("a");
        let _ = q.get(0);
        q.done("a");
        assert!(q.is_empty());
    }

    #[test]
    fn add_after_is_not_ready_until_due() {
        let mut q = WorkQueue::new(cfg());
        q.add_after("a", 100, 0);
        assert_eq!(q.delayed_len(), 1);
        assert_eq!(q.get(50), None, "not yet due");
        assert_eq!(q.get(99), None, "still not due");
        assert_eq!(q.get(100).as_deref(), Some("a"), "due at exactly ready_at");
    }

    #[test]
    fn add_after_zero_delay_is_immediate() {
        let mut q = WorkQueue::new(cfg());
        q.add_after("a", 0, 5);
        assert_eq!(q.ready_len(), 1);
    }

    #[test]
    fn add_after_keeps_earliest_ready_time() {
        let mut q = WorkQueue::new(cfg());
        q.add_after("a", 100, 0);
        q.add_after("a", 50, 0); // earlier wins
        assert_eq!(q.delayed_len(), 1);
        assert_eq!(q.get(60).as_deref(), Some("a"));
    }

    #[test]
    fn add_after_ignored_if_later_than_existing() {
        let mut q = WorkQueue::new(cfg());
        q.add_after("a", 50, 0);
        q.add_after("a", 100, 0); // later: ignored
        assert_eq!(q.delayed_len(), 1);
        assert_eq!(q.get(50).as_deref(), Some("a"));
    }

    #[test]
    fn immediate_add_beats_pending_delay() {
        let mut q = WorkQueue::new(cfg());
        q.add_after("a", 100, 0);
        q.add("a"); // becomes ready now; the delayed copy must collapse
        assert_eq!(q.get(0).as_deref(), Some("a"));
        // Draining the delayed bucket later must not double-deliver.
        assert_eq!(q.get(200), None);
    }

    #[test]
    fn delayed_items_become_ready_in_due_order() {
        let mut q = WorkQueue::new(cfg());
        q.add_after("late", 100, 0);
        q.add_after("early", 10, 0);
        assert_eq!(q.get(100).as_deref(), Some("early"));
        assert_eq!(q.get(100).as_deref(), Some("late"));
    }

    #[test]
    fn backoff_curve_doubles_per_failure() {
        let c = cfg(); // base 10, cap 1000
        assert_eq!(c.backoff_for(1), 10);
        assert_eq!(c.backoff_for(2), 20);
        assert_eq!(c.backoff_for(3), 40);
        assert_eq!(c.backoff_for(4), 80);
        assert_eq!(c.backoff_for(5), 160);
    }

    #[test]
    fn backoff_curve_caps_at_max_delay() {
        let c = cfg(); // cap 1000
        assert_eq!(c.backoff_for(7), 640);
        assert_eq!(c.backoff_for(8), 1000, "1280 clamped to 1000");
        assert_eq!(c.backoff_for(40), 1000, "huge shift clamps to cap");
        assert_eq!(c.backoff_for(200), 1000, "shift beyond 64 clamps to cap");
    }

    #[test]
    fn backoff_for_zero_is_base() {
        assert_eq!(cfg().backoff_for(0), 10);
    }

    #[test]
    fn rate_limited_add_schedules_growing_delays() {
        let mut q = WorkQueue::new(cfg());
        let o1 = q.add_rate_limited("a", 0);
        assert_eq!(o1, AddOutcome::Requeued { failures: 1, delay: 10 });
        // Becomes ready at 10; drain it so the next add can reschedule.
        assert_eq!(q.get(10).as_deref(), Some("a"));
        q.done("a");
        let o2 = q.add_rate_limited("a", 10);
        assert_eq!(o2, AddOutcome::Requeued { failures: 2, delay: 20 });
        assert_eq!(q.get(20), None, "ready at 10+20=30");
        assert_eq!(q.get(30).as_deref(), Some("a"));
    }

    #[test]
    fn rate_limited_add_drops_after_max_retries() {
        let mut q = WorkQueue::new(cfg()); // max_retries = 3
        for n in 1..=3 {
            let o = q.add_rate_limited("a", 0);
            assert!(matches!(o, AddOutcome::Requeued { failures, .. } if failures == n));
            // drain so each re-add reschedules cleanly
            let _ = q.get(1_000_000);
            q.done("a");
        }
        let dropped = q.add_rate_limited("a", 0);
        assert_eq!(dropped, AddOutcome::Dropped { failures: 4 });
        assert!(q.is_empty(), "dropped key is not requeued");
        assert_eq!(q.retries("a"), 0, "history forgotten on drop");
    }

    #[test]
    fn forget_resets_backoff() {
        let mut q = WorkQueue::new(cfg());
        let _ = q.add_rate_limited("a", 0);
        let _ = q.get(1_000_000);
        q.done("a");
        let _ = q.add_rate_limited("a", 0);
        assert_eq!(q.retries("a"), 2);
        q.forget("a");
        assert_eq!(q.retries("a"), 0);
        let _ = q.get(1_000_000);
        q.done("a");
        let after = q.add_rate_limited("a", 0);
        assert_eq!(after, AddOutcome::Requeued { failures: 1, delay: 10 });
    }

    #[test]
    fn len_counts_ready_and_delayed_but_not_inflight() {
        let mut q = WorkQueue::new(cfg());
        q.add("ready");
        q.add_after("later", 100, 0);
        assert_eq!(q.len(), 2);
        let _ = q.get(0); // pulls "ready" in-flight
        assert_eq!(q.len(), 1, "in-flight not counted");
    }
}
