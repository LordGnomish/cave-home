// SPDX-License-Identifier: Apache-2.0
//! Debounced sync runner — port of upstream
//! `k8s.io/apimachinery/pkg/util/async.BoundedFrequencyRunner` /
//! `pkg/proxy/iptables/proxier.go syncRunner`.
//!
//! Phase 1 implementation: simple time-based debounce + periodic resync.
//! Backoff on iptables-restore error is wired in `proxier.rs` (caller).

use std::time::Duration;

/// Subset of `BoundedFrequencyRunner` knobs upstream uses; trimmed to the
/// three we care about for Phase 1.
#[derive(Debug, Clone, Copy)]
pub struct BoundedFrequencyConfig {
    /// Minimum interval between two consecutive syncs (rate-limit).
    pub min_interval: Duration,
    /// Debounce window — after the first event, wait this long before
    /// firing the sync to coalesce a burst.
    pub debounce: Duration,
    /// Periodic full resync period (upstream `proxyutil.FullSyncPeriod` = 1h).
    pub resync_period: Duration,
}

impl Default for BoundedFrequencyConfig {
    fn default() -> Self {
        Self {
            min_interval: Duration::from_millis(10),
            debounce: Duration::from_millis(10),
            resync_period: Duration::from_secs(3600),
        }
    }
}
