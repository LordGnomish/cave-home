// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Exponential reconnect backoff for the WebSocket subscription.
//!
//! Deterministic (no jitter): the delay is `base * 2^attempt`, saturated at
//! `max`. The client resets it on every successful (re)connection so a flaky
//! link doesn't permanently inflate the wait.

use std::time::Duration;

/// A capped, doubling backoff.
#[derive(Debug, Clone)]
pub struct Backoff {
    base: Duration,
    max: Duration,
    attempt: u32,
}

impl Backoff {
    /// Build a backoff that starts at `base` and never exceeds `max`.
    pub const fn new(base: Duration, max: Duration) -> Self {
        Self {
            base,
            max,
            attempt: 0,
        }
    }

    /// The next delay, then advance the attempt counter.
    pub fn next_delay(&mut self) -> Duration {
        let factor = 1u64.checked_shl(self.attempt).unwrap_or(u64::MAX);
        let base_ms = u64::try_from(self.base.as_millis()).unwrap_or(u64::MAX);
        let delay_ms = base_ms.saturating_mul(factor);
        let capped = delay_ms.min(u64::try_from(self.max.as_millis()).unwrap_or(u64::MAX));
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_millis(capped)
    }

    /// Reset to the base delay (call after a successful connection).
    pub const fn reset(&mut self) {
        self.attempt = 0;
    }

    /// The number of delays handed out since the last reset.
    pub const fn attempt(&self) -> u32 {
        self.attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn first_delay_is_base() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        assert_eq!(b.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn delay_doubles_each_attempt() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        assert_eq!(b.next_delay(), Duration::from_secs(1));
        assert_eq!(b.next_delay(), Duration::from_secs(2));
        assert_eq!(b.next_delay(), Duration::from_secs(4));
    }

    #[test]
    fn caps_at_max() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(5));
        for _ in 0..10 {
            assert!(b.next_delay() <= Duration::from_secs(5));
        }
        assert_eq!(b.next_delay(), Duration::from_secs(5));
    }

    #[test]
    fn reset_returns_to_base() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        b.next_delay();
        b.next_delay();
        b.reset();
        assert_eq!(b.attempt(), 0);
        assert_eq!(b.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn attempt_counts_up() {
        let mut b = Backoff::new(Duration::from_millis(100), Duration::from_secs(1));
        assert_eq!(b.attempt(), 0);
        b.next_delay();
        assert_eq!(b.attempt(), 1);
    }
}
