// SPDX-License-Identifier: Apache-2.0
//! Injectable clock so the lease TTL renewal loop is deterministic in tests.
//!
//! Upstream uses `time.Now()` directly; we factor it out per Cave golden rule 2
//! (TDD strict).

use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync + 'static {
    /// UNIX seconds.
    fn now(&self) -> u64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MockClock {
    inner: Arc<Mutex<u64>>,
}

impl MockClock {
    #[must_use]
    pub fn new(start: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(start)),
        }
    }

    pub fn advance(&self, secs: u64) {
        *self.inner.lock() += secs;
    }

    pub fn set(&self, t: u64) {
        *self.inner.lock() = t;
    }
}

impl Clock for MockClock {
    fn now(&self) -> u64 {
        *self.inner.lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_clock_advances() {
        let c = MockClock::new(100);
        assert_eq!(c.now(), 100);
        c.advance(50);
        assert_eq!(c.now(), 150);
        c.set(0);
        assert_eq!(c.now(), 0);
    }

    #[test]
    fn system_clock_is_nonzero() {
        let c = SystemClock;
        assert!(c.now() > 0);
    }
}
