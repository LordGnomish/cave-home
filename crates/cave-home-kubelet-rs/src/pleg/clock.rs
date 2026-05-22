// SPDX-License-Identifier: Apache-2.0
//! `Clock` abstraction so that nothing in production calls `Instant::now()`
//! directly. Hand-port of `staging/src/k8s.io/utils/clock/`.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Pluggable clock — tests inject `MockClock` so PLEG diffs are deterministic.
pub trait Clock: Send + Sync {
    /// Wall-clock unix-millis.
    fn now_unix_millis(&self) -> i64;
}

/// Real wall clock.
#[derive(Default)]
pub struct SystemClock;

impl SystemClock {
    pub fn new() -> Self {
        Self
    }
}

impl Clock for SystemClock {
    fn now_unix_millis(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            // i64 cast is safe: now is well below 2^63 ms.
            Ok(d) => d.as_millis() as i64,
            // Pre-1970 system clock — rare but well-defined: return 0.
            Err(_) => 0,
        }
    }
}

/// Test clock — explicit `set` / `advance` helpers.
pub struct MockClock {
    millis: AtomicI64,
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new(0)
    }
}

impl MockClock {
    pub fn new(initial_millis: i64) -> Self {
        Self {
            millis: AtomicI64::new(initial_millis),
        }
    }

    pub fn set(&self, millis: i64) {
        self.millis.store(millis, Ordering::SeqCst);
    }

    pub fn advance(&self, by_millis: i64) {
        self.millis.fetch_add(by_millis, Ordering::SeqCst);
    }
}

impl Clock for MockClock {
    fn now_unix_millis(&self) -> i64 {
        self.millis.load(Ordering::SeqCst)
    }
}
