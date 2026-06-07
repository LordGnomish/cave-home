// SPDX-License-Identifier: Apache-2.0
//! Retry policy for backend forwarding.
//!
//! Spec basis: Traefik's `Retry` middleware reissues a request a bounded number
//! of times when the backend cannot be reached (a *network* failure, before any
//! response is received), with an optional exponential backoff between attempts.
//! A request that produced an HTTP response — even a 5xx — is not retried.
//!
//! All decisions here are pure functions of the attempt count and the outcome
//! classification, so they are testable without a network or a clock.

/// What happened on a single forward attempt, for retry classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A response (any status) was received — never retry.
    Responded,
    /// The connection could not be established.
    ConnectFailed,
    /// The attempt timed out before a response.
    Timeout,
    /// The connection was reset / closed before a response.
    Reset,
}

/// Whether `outcome` is a retryable network failure.
#[must_use]
pub const fn is_retryable(outcome: Outcome) -> bool {
    matches!(outcome, Outcome::ConnectFailed | Outcome::Timeout | Outcome::Reset)
}

/// A bounded retry policy with exponential backoff.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Total number of attempts permitted (≥ 1; 1 means no retry).
    pub attempts: u32,
    /// Backoff before the first retry, doubled each subsequent retry.
    pub initial_interval_ms: u64,
    /// Upper bound on a single backoff interval.
    pub max_interval_ms: u64,
}

impl RetryPolicy {
    /// A policy that never retries (one attempt, no backoff).
    #[must_use]
    pub const fn none() -> Self {
        Self { attempts: 1, initial_interval_ms: 0, max_interval_ms: 0 }
    }

    /// Whether another attempt is permitted given `attempts_made` so far (≥ 1).
    #[must_use]
    pub const fn should_retry(&self, attempts_made: u32) -> bool {
        attempts_made < self.attempts
    }

    /// The backoff before the retry that follows `attempts_made` attempts:
    /// `initial · 2^(attempts_made-1)`, capped at `max_interval_ms`.
    #[must_use]
    pub fn backoff_ms(&self, attempts_made: u32) -> u64 {
        if attempts_made == 0 {
            return 0;
        }
        let shift = attempts_made - 1;
        let scaled = self
            .initial_interval_ms
            .checked_shl(shift)
            .unwrap_or(u64::MAX);
        scaled.min(self.max_interval_ms)
    }
}

/// The index of the next backend to try, rotating through `n` servers.
#[must_use]
pub const fn next_server_index(current: usize, n: usize) -> usize {
    if n == 0 {
        0
    } else {
        (current + 1) % n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_network_failures_are_retryable() {
        assert!(is_retryable(Outcome::ConnectFailed));
        assert!(is_retryable(Outcome::Timeout));
        assert!(is_retryable(Outcome::Reset));
        assert!(!is_retryable(Outcome::Responded));
    }

    #[test]
    fn should_retry_respects_attempt_budget() {
        let p = RetryPolicy { attempts: 3, initial_interval_ms: 100, max_interval_ms: 2000 };
        assert!(p.should_retry(1));
        assert!(p.should_retry(2));
        assert!(!p.should_retry(3)); // 3 attempts made == budget, stop
    }

    #[test]
    fn none_policy_never_retries() {
        let p = RetryPolicy::none();
        assert_eq!(p.attempts, 1);
        assert!(!p.should_retry(1));
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let p = RetryPolicy { attempts: 5, initial_interval_ms: 100, max_interval_ms: 500 };
        assert_eq!(p.backoff_ms(1), 100); // 100 * 2^0
        assert_eq!(p.backoff_ms(2), 200); // 100 * 2^1
        assert_eq!(p.backoff_ms(3), 400); // 100 * 2^2
        assert_eq!(p.backoff_ms(4), 500); // 800 capped to 500
    }

    #[test]
    fn next_server_rotates() {
        assert_eq!(next_server_index(0, 3), 1);
        assert_eq!(next_server_index(2, 3), 0);
        assert_eq!(next_server_index(0, 1), 0);
        assert_eq!(next_server_index(5, 0), 0); // degenerate: no servers
    }
}
