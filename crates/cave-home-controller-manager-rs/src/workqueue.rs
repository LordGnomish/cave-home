// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         staging/src/k8s.io/client-go/util/workqueue/{queue.go,rate_limiting_queue.go,default_rate_limiters.go}
//
//! Rate-limited workqueue.
//!
//! Mirrors `client-go/util/workqueue` — every controller in `pkg/controller/`
//! reaches for this type. Three layers (queue / delayed / rate-limited) are
//! collapsed into one struct because the consumer interface in Phase 2 only
//! needs `add`, `add_after`, `add_rate_limited`, `get`, `done`, and `forget`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use parking_lot::Mutex;
use tokio::sync::Notify;

/// Rate-limit policy applied by [`RateLimitingQueue::add_rate_limited`].
///
/// Mirrors `workqueue.ItemExponentialFailureRateLimiter`: per-key counter
/// doubles on each `add_rate_limited`, capped by `max_delay`. The counter
/// resets to zero on [`RateLimitingQueue::forget`].
#[derive(Clone, Debug)]
pub struct ExponentialBackoff {
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        // Same defaults as `workqueue.DefaultControllerRateLimiter`:
        // 5 ms initial -> capped at 1000 s.
        Self {
            base_delay: Duration::from_millis(5),
            max_delay: Duration::from_secs(1000),
        }
    }
}

impl ExponentialBackoff {
    #[must_use]
    pub fn when(&self, failures: u32) -> Duration {
        // 2^failures * base, with overflow saturation matching upstream.
        let factor = 1u64.checked_shl(failures).unwrap_or(u64::MAX);
        let base_ms = self.base_delay.as_millis() as u64;
        let delay_ms = base_ms.saturating_mul(factor);
        let capped = std::cmp::min(delay_ms, self.max_delay.as_millis() as u64);
        Duration::from_millis(capped)
    }
}

/// Generic rate-limited workqueue.
///
/// `T` is the item type — for every Phase 2 controller this is a string key
/// of the form `"namespace/name"`. We keep the type parameter so the
/// node-controller (which keys on bare node name) doesn't have to fabricate
/// a fake namespace.
pub struct RateLimitingQueue<T: Clone + Eq + Hash + Send + Sync + 'static> {
    inner: Arc<Mutex<Inner<T>>>,
    notify: Arc<Notify>,
    rate_limiter: ExponentialBackoff,
}

struct Inner<T: Clone + Eq + Hash> {
    /// FIFO of items currently ready to be processed.
    queue: VecDeque<T>,
    /// Items in `queue` OR currently being processed (between `get` and `done`).
    dirty: HashSet<T>,
    /// Items currently being processed.
    processing: HashSet<T>,
    /// Per-key failure counter, used by `add_rate_limited`.
    failures: HashMap<T, u32>,
    /// Items scheduled to be enqueued at `(when, item)`. Sorted ascending by
    /// `when` for cheap drain.
    delayed: Vec<(Instant, T)>,
    shutdown: bool,
}

impl<T: Clone + Eq + Hash + Send + Sync + 'static> Default for RateLimitingQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Eq + Hash + Send + Sync + 'static> Clone for RateLimitingQueue<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            notify: Arc::clone(&self.notify),
            rate_limiter: self.rate_limiter.clone(),
        }
    }
}

impl<T: Clone + Eq + Hash + Send + Sync + 'static> RateLimitingQueue<T> {
    #[must_use]
    pub fn new() -> Self {
        Self::with_rate_limiter(ExponentialBackoff::default())
    }

    #[must_use]
    pub fn with_rate_limiter(rate_limiter: ExponentialBackoff) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                queue: VecDeque::new(),
                dirty: HashSet::new(),
                processing: HashSet::new(),
                failures: HashMap::new(),
                delayed: Vec::new(),
                shutdown: false,
            })),
            notify: Arc::new(Notify::new()),
            rate_limiter,
        }
    }

    /// `Add(item)` — enqueue exactly once, deduplicated.
    pub fn add(&self, item: T) {
        let mut inner = self.inner.lock();
        if inner.shutdown {
            return;
        }
        if inner.dirty.contains(&item) {
            // Already queued or in flight — coalesce.
            return;
        }
        inner.dirty.insert(item.clone());
        if inner.processing.contains(&item) {
            // It's currently being processed; mark dirty so `done` requeues.
            return;
        }
        inner.queue.push_back(item);
        self.notify.notify_one();
    }

    /// `AddAfter(item, duration)` — enqueue after `duration` elapses.
    pub fn add_after(&self, item: T, duration: Duration) {
        if duration.is_zero() {
            self.add(item);
            return;
        }
        let mut inner = self.inner.lock();
        if inner.shutdown {
            return;
        }
        let when = Instant::now() + duration;
        inner.delayed.push((when, item));
        // Keep `delayed` weakly sorted (insertion sort over the tail). The
        // pop path scans linearly so this is purely a fairness optimisation.
        inner.delayed.sort_by_key(|(t, _)| *t);
    }

    /// `AddRateLimited(item)` — enqueue with backoff based on per-key failures.
    pub fn add_rate_limited(&self, item: T) {
        let delay = {
            let mut inner = self.inner.lock();
            let failures = inner.failures.entry(item.clone()).or_insert(0);
            *failures += 1;
            self.rate_limiter.when(*failures - 1)
        };
        self.add_after(item, delay);
    }

    /// Drain any delayed items whose deadline is in the past. Internal helper
    /// used by both `get` and `len`.
    fn drain_ready(&self, now: Instant) {
        let mut inner = self.inner.lock();
        let mut still_delayed = Vec::with_capacity(inner.delayed.len());
        let drained: Vec<T> = std::mem::take(&mut inner.delayed)
            .into_iter()
            .filter_map(|(when, item)| {
                if when <= now {
                    Some(item)
                } else {
                    still_delayed.push((when, item));
                    None
                }
            })
            .collect();
        inner.delayed = still_delayed;
        for item in drained {
            if inner.dirty.contains(&item) {
                continue;
            }
            inner.dirty.insert(item.clone());
            if inner.processing.contains(&item) {
                continue;
            }
            inner.queue.push_back(item);
            self.notify.notify_one();
        }
    }

    /// `Get()` — pop the next ready item. Returns `None` if shutdown.
    ///
    /// Blocks asynchronously when the queue is empty.
    pub async fn get(&self) -> Option<T> {
        loop {
            self.drain_ready(Instant::now());
            // Drop the lock before awaiting. parking_lot::MutexGuard is !Send,
            // so we narrow the critical section.
            let sleep_target;
            {
                let mut inner = self.inner.lock();
                if inner.shutdown && inner.queue.is_empty() && inner.delayed.is_empty() {
                    return None;
                }
                if let Some(item) = inner.queue.pop_front() {
                    inner.processing.insert(item.clone());
                    inner.dirty.remove(&item);
                    return Some(item);
                }
                sleep_target = inner.delayed.iter().map(|(t, _)| *t).min();
            }
            // Wait either for a notify (new item) or until the next delayed
            // item is due.
            let notify = Arc::clone(&self.notify);
            if let Some(t) = sleep_target {
                let now = Instant::now();
                if t > now {
                    let _ = tokio::time::timeout(t - now, notify.notified()).await;
                }
            } else {
                notify.notified().await;
            }
        }
    }

    /// Try to pop an item without awaiting. Returns `None` when the queue is
    /// momentarily empty.
    pub fn try_get(&self) -> Option<T> {
        self.drain_ready(Instant::now());
        let mut inner = self.inner.lock();
        if let Some(item) = inner.queue.pop_front() {
            inner.processing.insert(item.clone());
            inner.dirty.remove(&item);
            return Some(item);
        }
        None
    }

    /// `Done(item)` — caller is finished with the item.
    ///
    /// If it was re-marked dirty while processing (via a second `add`), it is
    /// re-enqueued now.
    pub fn done(&self, item: &T) {
        let mut inner = self.inner.lock();
        inner.processing.remove(item);
        if inner.dirty.contains(item) {
            // Was marked dirty during processing? Re-queue. (Dirty is kept
            // until Get drains again.)
            inner.queue.push_back(item.clone());
            self.notify.notify_one();
        }
    }

    /// `Forget(item)` — clear failure counter (call after a successful sync).
    pub fn forget(&self, item: &T) {
        let mut inner = self.inner.lock();
        inner.failures.remove(item);
    }

    /// `Len()` — best-effort visible queue length (ready + processing).
    pub fn len(&self) -> usize {
        self.drain_ready(Instant::now());
        let inner = self.inner.lock();
        inner.queue.len() + inner.processing.len()
    }

    /// `Empty()` convenience.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `ShutDown()` — refuse further adds and let pending `get`s drain.
    pub fn shutdown(&self) {
        self.inner.lock().shutdown = true;
        self.notify.notify_waiters();
    }

    /// Snapshot of the failure counter for `item` — observability only.
    pub fn failures(&self, item: &T) -> u32 {
        self.inner.lock().failures.get(item).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn add_get_done_round_trip() {
        let q: RateLimitingQueue<String> = RateLimitingQueue::new();
        q.add("a".into());
        let got = q.get().await.unwrap();
        assert_eq!(got, "a");
        q.done(&got);
        q.shutdown();
        assert!(q.get().await.is_none());
    }

    #[tokio::test]
    async fn add_is_deduplicated() {
        let q: RateLimitingQueue<String> = RateLimitingQueue::new();
        q.add("a".into());
        q.add("a".into());
        q.add("a".into());
        assert_eq!(q.try_get(), Some("a".into()));
        assert_eq!(q.try_get(), None);
    }

    #[tokio::test]
    async fn add_during_processing_requeues_on_done() {
        let q: RateLimitingQueue<String> = RateLimitingQueue::new();
        q.add("a".into());
        let item = q.get().await.unwrap();
        // Second add lands while item is in-flight.
        q.add("a".into());
        // Until done, get blocks (and the second add did not produce a ready
        // item).
        assert_eq!(q.try_get(), None);
        q.done(&item);
        let again = q.get().await.unwrap();
        assert_eq!(again, "a");
    }

    #[tokio::test(start_paused = true)]
    async fn add_after_delays_visibility() {
        let q: RateLimitingQueue<String> = RateLimitingQueue::new();
        q.add_after("a".into(), Duration::from_millis(50));
        assert_eq!(q.try_get(), None);
        tokio::time::advance(Duration::from_millis(60)).await;
        // After advancing past the deadline a `try_get` makes the item visible.
        assert_eq!(q.try_get(), Some("a".into()));
    }

    #[test]
    fn exponential_backoff_doubles_until_cap() {
        let r = ExponentialBackoff {
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(160),
        };
        assert_eq!(r.when(0), Duration::from_millis(10));
        assert_eq!(r.when(1), Duration::from_millis(20));
        assert_eq!(r.when(2), Duration::from_millis(40));
        assert_eq!(r.when(3), Duration::from_millis(80));
        assert_eq!(r.when(4), Duration::from_millis(160));
        // Capped beyond.
        assert_eq!(r.when(50), Duration::from_millis(160));
    }

    #[test]
    fn forget_clears_failures() {
        let q: RateLimitingQueue<String> = RateLimitingQueue::new();
        q.add_rate_limited("a".into());
        q.add_rate_limited("a".into());
        assert_eq!(q.failures(&"a".to_string()), 2);
        q.forget(&"a".to_string());
        assert_eq!(q.failures(&"a".to_string()), 0);
    }

    #[tokio::test]
    async fn shutdown_unblocks_get() {
        let q: RateLimitingQueue<String> = RateLimitingQueue::new();
        let q2 = q.clone();
        let handle = tokio::spawn(async move { q2.get().await });
        // Give the spawned task a chance to park.
        tokio::task::yield_now().await;
        q.shutdown();
        let got = handle.await.unwrap();
        assert!(got.is_none());
    }
}
