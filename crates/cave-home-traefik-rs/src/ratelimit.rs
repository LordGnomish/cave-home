// SPDX-License-Identifier: Apache-2.0
//! Token-bucket rate limiting.
//!
//! Spec basis: Traefik's `RateLimit` middleware enforces a sustained `average`
//! rate with a `burst` allowance via a token bucket. This implements the bucket
//! with integer "milli-token" arithmetic (1 token = 1000 milli-tokens) so there
//! is no floating-point rounding, and a caller-supplied millisecond clock so it
//! is deterministic and testable.
//!
//! An `average` of 0 means "no limit" (every request is admitted), matching
//! Traefik's treatment of an unset rate.

use std::collections::HashMap;

const MILLI: u64 = 1000;

/// A single token bucket.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    capacity_milli: u64,
    refill_milli_per_ms: u64,
    tokens_milli: u64,
    last_ms: u64,
    unlimited: bool,
}

impl TokenBucket {
    /// A bucket admitting `average` requests/second sustained with a `burst`
    /// capacity. `average == 0` disables the limit.
    #[must_use]
    pub fn new(average_per_sec: u64, burst: u64) -> Self {
        unimplemented!()
    }

    /// Refill according to elapsed time, then try to consume one token.
    /// Returns `true` if the request is admitted.
    pub fn allow(&mut self, now_ms: u64) -> bool {
        unimplemented!()
    }

    /// Current whole-token count (for inspection / metrics).
    #[must_use]
    pub fn tokens(&self) -> u64 {
        self.tokens_milli / MILLI
    }
}

/// A keyed set of token buckets (e.g. one per client source), all sharing the
/// same average/burst configuration.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    average_per_sec: u64,
    burst: u64,
    buckets: HashMap<String, TokenBucket>,
}

impl RateLimiter {
    /// A limiter that mints a fresh bucket per key on first sight.
    #[must_use]
    pub fn new(average_per_sec: u64, burst: u64) -> Self {
        unimplemented!()
    }

    /// Admit (or reject) a request from `key` at `now_ms`.
    pub fn allow(&mut self, key: &str, now_ms: u64) -> bool {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_allows_then_blocks() {
        let mut b = TokenBucket::new(10, 3);
        assert!(b.allow(0));
        assert!(b.allow(0));
        assert!(b.allow(0));
        assert!(!b.allow(0)); // burst of 3 exhausted
    }

    #[test]
    fn refills_over_time() {
        let mut b = TokenBucket::new(10, 3);
        for _ in 0..3 {
            b.allow(0);
        }
        assert!(!b.allow(0));
        // 10 tokens/sec => 1 token per 100 ms
        assert!(b.allow(100));
        assert!(!b.allow(100));
    }

    #[test]
    fn capacity_caps_accumulation() {
        let mut b = TokenBucket::new(10, 3);
        // idle a long time: tokens cap at burst, not unbounded
        assert!(b.allow(1_000_000));
        assert!(b.allow(1_000_000));
        assert!(b.allow(1_000_000));
        assert!(!b.allow(1_000_000));
    }

    #[test]
    fn zero_average_is_unlimited() {
        let mut b = TokenBucket::new(0, 0);
        for t in 0..100 {
            assert!(b.allow(t));
        }
    }

    #[test]
    fn limiter_keys_are_independent() {
        let mut rl = RateLimiter::new(10, 1);
        assert!(rl.allow("a", 0));
        assert!(!rl.allow("a", 0)); // a exhausted
        assert!(rl.allow("b", 0)); // b has its own bucket
    }
}
