//! De-duplication and rate limiting — keeping a household from being spammed.
//!
//! Two independent guards, both modelled as pure logic over caller-supplied
//! state and a caller-supplied integer clock (no time crate, Charter pure-logic
//! rule):
//!
//! - [`DedupCache`] collapses *identical* notifications (same topic+title+body,
//!   the [`DedupKey`](crate::notification::DedupKey)) seen within a window. A
//!   re-fire of the same alert one second later is suppressed; the same alert an
//!   hour later is allowed through again.
//! - [`RateLimiter`] is a token bucket *per topic*: a topic refills at a steady
//!   rate up to a burst ceiling, and each notification spends one token. When
//!   the bucket is empty the notification is throttled until it refills.
//!
//! Both take the current [`Tick`] from the caller and never read the clock
//! themselves, so they are deterministic and trivially testable.

use std::collections::HashMap;

use crate::notification::{DedupKey, Notification, Tick};

/// A small de-duplication cache keyed by notification content.
///
/// "Within `window` ticks of an identical notification" counts as a duplicate.
#[derive(Debug, Clone)]
pub struct DedupCache {
    window: Tick,
    last_seen: HashMap<DedupKey, Tick>,
}

impl DedupCache {
    /// Build a cache that suppresses repeats seen within `window` ticks.
    ///
    /// A `window` of `0` disables suppression (every notification is fresh).
    #[must_use]
    pub fn new(window: Tick) -> Self {
        Self {
            window,
            last_seen: HashMap::new(),
        }
    }

    /// Offer a notification to the cache.
    ///
    /// Returns `true` if it should be delivered (it is new, or the previous
    /// identical one is older than the window) and records it. Returns `false`
    /// if it is a duplicate within the window — in which case nothing is
    /// recorded and the original window is preserved.
    pub fn admit(&mut self, n: &Notification) -> bool {
        let now = n.created_at();
        let key = n.dedup_key();
        if let Some(&prev) = self.last_seen.get(&key) {
            // Saturating: a clock that went backwards never under-flows; such a
            // notification is treated as within-window (suppressed).
            let elapsed = now.saturating_sub(prev);
            if elapsed < self.window {
                return false;
            }
        }
        self.last_seen.insert(key, now);
        true
    }

    /// Drop entries last seen more than `window` ticks before `now`, freeing
    /// memory for keys that can no longer cause a suppression. Pure maintenance;
    /// it never changes an [`admit`](Self::admit) outcome.
    pub fn prune(&mut self, now: Tick) {
        let window = self.window;
        self.last_seen
            .retain(|_, &mut seen| now.saturating_sub(seen) < window);
    }

    /// How many distinct keys are currently remembered.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.last_seen.len()
    }
}

/// A per-topic token bucket.
///
/// Each topic gets `burst` tokens to start; tokens refill at one per
/// `refill_interval` ticks up to the `burst` ceiling. Each admitted
/// notification spends one token.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    burst: u32,
    refill_interval: Tick,
    buckets: HashMap<String, Bucket>,
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: u32,
    last_refill: Tick,
}

impl RateLimiter {
    /// A limiter giving each topic `burst` tokens, refilling one token every
    /// `refill_interval` ticks.
    ///
    /// A `refill_interval` of `0` means "never refill" — the bucket is a pure
    /// quota of `burst` notifications per topic.
    #[must_use]
    pub fn new(burst: u32, refill_interval: Tick) -> Self {
        Self {
            burst,
            refill_interval,
            buckets: HashMap::new(),
        }
    }

    /// Offer a notification to the limiter at its creation tick.
    ///
    /// Returns `true` and spends a token if the topic's bucket has one;
    /// otherwise returns `false` and spends nothing.
    pub fn admit(&mut self, n: &Notification) -> bool {
        let now = n.created_at();
        let topic = n.topic().as_str().to_owned();
        let burst = self.burst;
        let interval = self.refill_interval;
        let bucket = self.buckets.entry(topic).or_insert(Bucket {
            tokens: burst,
            last_refill: now,
        });

        // `checked_div` yields None for a zero interval ("never refill"); the
        // `> 0` filter skips the case where not enough time has passed yet.
        let elapsed = now.saturating_sub(bucket.last_refill);
        if let Some(gained) = elapsed.checked_div(interval).filter(|&g| g > 0) {
            // Saturate the add and clamp to the burst ceiling.
            let gained_u32 = u32::try_from(gained).unwrap_or(u32::MAX);
            bucket.tokens = bucket.tokens.saturating_add(gained_u32).min(burst);
            // Advance the refill clock by the consumed intervals only, so
            // leftover fractional time still counts toward the next token.
            bucket.last_refill = bucket.last_refill.saturating_add(gained * interval);
        }

        if bucket.tokens > 0 {
            bucket.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Tokens currently available for a topic (after no further refill), for
    /// inspection / tests. A topic never seen yet reports its full `burst`.
    #[must_use]
    pub fn available(&self, topic: &str) -> u32 {
        self.buckets.get(topic).map_or(self.burst, |b| b.tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::priority::Priority;
    use crate::topic::Topic;

    fn topic(name: &str) -> Topic {
        Topic::new(name).expect("valid test topic")
    }

    fn note_at(t: &str, tick: Tick) -> Notification {
        Notification::new(topic(t), "Leak", "Kitchen", tick).with_priority(Priority::High)
    }

    // ---- dedup ----

    #[test]
    fn dedup_suppresses_identical_within_window() {
        let mut cache = DedupCache::new(60);
        assert!(cache.admit(&note_at("leak", 0)));
        // 30s later, same content -> suppressed.
        assert!(!cache.admit(&note_at("leak", 30)));
        assert!(!cache.admit(&note_at("leak", 59)));
    }

    #[test]
    fn dedup_allows_after_window_boundary() {
        let mut cache = DedupCache::new(60);
        assert!(cache.admit(&note_at("leak", 0)));
        // Exactly at the window edge: elapsed == window is NOT "within" -> allow.
        assert!(cache.admit(&note_at("leak", 60)));
        // And that re-arms the window from 60.
        assert!(!cache.admit(&note_at("leak", 90)));
    }

    #[test]
    fn dedup_zero_window_never_suppresses() {
        let mut cache = DedupCache::new(0);
        assert!(cache.admit(&note_at("leak", 0)));
        assert!(cache.admit(&note_at("leak", 0)));
    }

    #[test]
    fn dedup_distinguishes_content() {
        let mut cache = DedupCache::new(1000);
        let a = Notification::new(topic("leak"), "Leak", "Kitchen", 0);
        let b = Notification::new(topic("leak"), "Leak", "Bathroom", 1);
        assert!(cache.admit(&a));
        assert!(cache.admit(&b)); // different body -> not a duplicate
    }

    #[test]
    fn dedup_prune_drops_stale_keys() {
        let mut cache = DedupCache::new(60);
        assert!(cache.admit(&note_at("leak", 0)));
        assert_eq!(cache.tracked(), 1);
        cache.prune(1000); // far past the window
        assert_eq!(cache.tracked(), 0);
    }

    #[test]
    fn dedup_handles_backwards_clock_as_within_window() {
        let mut cache = DedupCache::new(60);
        assert!(cache.admit(&note_at("leak", 100)));
        // A notification stamped earlier than the last seen is suppressed
        // rather than panicking on underflow.
        assert!(!cache.admit(&note_at("leak", 50)));
    }

    // ---- rate limit ----

    #[test]
    fn rate_limit_spends_the_burst_then_throttles() {
        let mut rl = RateLimiter::new(3, 10);
        assert!(rl.admit(&note_at("leak", 0)));
        assert!(rl.admit(&note_at("leak", 0)));
        assert!(rl.admit(&note_at("leak", 0)));
        // Burst exhausted, no time passed -> throttled.
        assert!(!rl.admit(&note_at("leak", 0)));
        assert_eq!(rl.available("leak"), 0);
    }

    #[test]
    fn rate_limit_refills_one_token_per_interval() {
        let mut rl = RateLimiter::new(1, 10);
        assert!(rl.admit(&note_at("leak", 0))); // spends the only token
        assert!(!rl.admit(&note_at("leak", 5))); // half an interval, no refill
        assert!(rl.admit(&note_at("leak", 10))); // one interval -> one token back
        assert!(!rl.admit(&note_at("leak", 11)));
    }

    #[test]
    fn rate_limit_refill_clamps_to_burst() {
        let mut rl = RateLimiter::new(2, 10);
        assert!(rl.admit(&note_at("leak", 0)));
        // A long idle gap would refill many tokens, but the ceiling is 2.
        assert!(rl.admit(&note_at("leak", 10_000)));
        assert!(rl.admit(&note_at("leak", 10_000)));
        assert!(!rl.admit(&note_at("leak", 10_000)));
    }

    #[test]
    fn rate_limit_is_per_topic() {
        let mut rl = RateLimiter::new(1, 10);
        assert!(rl.admit(&note_at("leak", 0)));
        assert!(!rl.admit(&note_at("leak", 0)));
        // A different topic has its own independent bucket.
        assert!(rl.admit(&note_at("door", 0)));
    }

    #[test]
    fn rate_limit_zero_interval_is_a_fixed_quota() {
        let mut rl = RateLimiter::new(2, 0);
        assert!(rl.admit(&note_at("leak", 0)));
        assert!(rl.admit(&note_at("leak", 1_000_000)));
        // Never refills, so the third is throttled no matter how much time passes.
        assert!(!rl.admit(&note_at("leak", u64::MAX)));
    }

    #[test]
    fn rate_limit_unseen_topic_reports_full_burst() {
        let rl = RateLimiter::new(5, 10);
        assert_eq!(rl.available("never-touched"), 5);
    }
}
