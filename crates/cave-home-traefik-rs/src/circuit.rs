// SPDX-License-Identifier: Apache-2.0
//! Circuit breaker for backend forwarding.
//!
//! Spec basis: Traefik's `CircuitBreaker` middleware trips a router open when a
//! failure measure crosses a threshold, refuses traffic for a recovery window,
//! then probes with a single request before fully closing again.
//!
//! This is the classic three-state breaker (Closed → Open → Half-Open),
//! modelled deterministically: the caller supplies a millisecond clock so the
//! recovery timing is testable without sleeping.

/// The breaker's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Forwarding normally; sampling outcomes.
    Closed,
    /// Tripped: refusing traffic until the recovery window elapses.
    Open,
    /// Probing: a single trial request is permitted to test recovery.
    HalfOpen,
}

/// A three-state circuit breaker keyed on a backend failure ratio.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    threshold: f64,
    min_requests: u64,
    recovery_ms: u64,
    state: State,
    successes: u64,
    failures: u64,
    opened_at: u64,
    probe_inflight: bool,
}

impl CircuitBreaker {
    /// Create a breaker that trips when the failure ratio reaches `threshold`
    /// (0.0–1.0) over at least `min_requests` samples, and stays open for
    /// `recovery_ms` before permitting a half-open probe.
    #[must_use]
    pub const fn new(threshold: f64, min_requests: u64, recovery_ms: u64) -> Self {
        Self {
            threshold,
            min_requests,
            recovery_ms,
            state: State::Closed,
            successes: 0,
            failures: 0,
            opened_at: 0,
            probe_inflight: false,
        }
    }

    /// The current state.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// Decide whether to forward a request now. May transition Open → Half-Open
    /// when the recovery window has elapsed (permitting exactly one probe).
    pub const fn allow(&mut self, now_ms: u64) -> bool {
        match self.state {
            State::Closed => true,
            State::Open => {
                if now_ms.saturating_sub(self.opened_at) >= self.recovery_ms {
                    self.state = State::HalfOpen;
                    self.probe_inflight = true;
                    true
                } else {
                    false
                }
            }
            State::HalfOpen => {
                if self.probe_inflight {
                    false
                } else {
                    self.probe_inflight = true;
                    true
                }
            }
        }
    }

    /// Record a successful forward.
    pub fn on_success(&mut self) {
        if self.state == State::HalfOpen {
            self.close();
        } else {
            self.successes += 1;
        }
    }

    /// Record a failed forward at `now_ms` (used to stamp the open time).
    pub fn on_failure(&mut self, now_ms: u64) {
        match self.state {
            State::HalfOpen => self.trip(now_ms),
            State::Closed => {
                self.failures += 1;
                let total = self.successes + self.failures;
                if total >= self.min_requests && self.failure_ratio() >= self.threshold {
                    self.trip(now_ms);
                }
            }
            State::Open => {}
        }
    }

    /// Failure ratio over the current sampling window.
    ///
    /// Counts are tiny (bounded by `min_requests` plus in-flight requests),
    /// far below 2^53, so the `u64 → f64` conversion is exact.
    #[allow(clippy::cast_precision_loss)]
    fn failure_ratio(&self) -> f64 {
        let total = self.successes + self.failures;
        if total == 0 {
            0.0
        } else {
            self.failures as f64 / total as f64
        }
    }

    const fn trip(&mut self, now_ms: u64) {
        self.state = State::Open;
        self.opened_at = now_ms;
        self.successes = 0;
        self.failures = 0;
        self.probe_inflight = false;
    }

    const fn close(&mut self) {
        self.state = State::Closed;
        self.successes = 0;
        self.failures = 0;
        self.probe_inflight = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_breaker_allows() {
        let mut cb = CircuitBreaker::new(0.5, 4, 1000);
        assert_eq!(cb.state(), State::Closed);
        assert!(cb.allow(0));
    }

    #[test]
    fn trips_open_when_failure_ratio_crosses_threshold() {
        let mut cb = CircuitBreaker::new(0.5, 4, 1000);
        cb.on_success();
        cb.on_success();
        cb.on_failure(0); // 2s/1f, total 3 < min 4 -> still closed
        assert_eq!(cb.state(), State::Closed);
        cb.on_failure(0); // total 4, ratio 2/4 = 0.5 -> open
        assert_eq!(cb.state(), State::Open);
        assert!(!cb.allow(0));
    }

    #[test]
    fn below_min_requests_never_trips() {
        let mut cb = CircuitBreaker::new(0.5, 10, 1000);
        for _ in 0..5 {
            cb.on_failure(0);
        }
        assert_eq!(cb.state(), State::Closed);
        assert!(cb.allow(0));
    }

    #[test]
    fn open_denies_until_recovery_then_half_open_probe() {
        let mut cb = CircuitBreaker::new(0.5, 2, 1000);
        cb.on_failure(0);
        cb.on_failure(0); // open at t=0
        assert!(!cb.allow(500)); // before recovery
        assert!(cb.allow(1000)); // recovery elapsed -> half-open probe permitted
        assert_eq!(cb.state(), State::HalfOpen);
        assert!(!cb.allow(1000)); // only one probe in flight
    }

    #[test]
    fn half_open_success_closes() {
        let mut cb = CircuitBreaker::new(0.5, 2, 1000);
        cb.on_failure(0);
        cb.on_failure(0);
        assert!(cb.allow(1000)); // half-open probe
        cb.on_success();
        assert_eq!(cb.state(), State::Closed);
        assert!(cb.allow(1000));
    }

    #[test]
    fn half_open_failure_reopens() {
        let mut cb = CircuitBreaker::new(0.5, 2, 1000);
        cb.on_failure(0);
        cb.on_failure(0);
        assert!(cb.allow(1000)); // half-open probe
        cb.on_failure(1000);
        assert_eq!(cb.state(), State::Open);
        assert!(!cb.allow(1500)); // re-armed recovery window
        assert!(cb.allow(2000));
    }
}
