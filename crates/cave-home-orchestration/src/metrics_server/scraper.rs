//! The scrape scheduling decision and per-node latency / error accounting.
//!
//! The `pkg/scraper` decision spine plus the manager's tick loop.
//!
//! metrics-server ticks every `metric-resolution` and scrapes each node's
//! kubelet with a timeout, recording the request duration, a success/failure
//! counter, and the last request time. This module is the pure decision half of
//! that loop: [`Scraper::due`] answers *which node should be scraped now*, and
//! [`Scraper::record`] folds each scrape [`ScrapeOutcome`] into the counters the
//! observability track exports — total scrapes, scrape **error rate**, and
//! scrape **latency**.
//!
//! There is no HTTP here and no clock: the caller performs the kubelet scrape,
//! supplies the monotonic `now` to [`Scraper::due`], and reports the outcome.
//! The transport and the concurrency/timeout enforcement are runtime-bound
//! (ADR-004 phase-1b).

use std::collections::BTreeMap;

/// The default scrape interval — metrics-server's `--metric-resolution` (15s).
pub const DEFAULT_RESOLUTION_NANOS: u64 = 15_000_000_000;

/// How a single kubelet scrape ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrapeFailure {
    /// The scrape exceeded the configured timeout budget.
    Timeout,
    /// The kubelet could not be reached (connection refused / no route).
    Unreachable,
    /// The response was reached but could not be decoded.
    Decode,
}

/// The result of a scrape: success, or a classified failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrapeResult {
    /// The kubelet answered and its summary decoded.
    Success,
    /// The scrape failed, with a cause.
    Failure(ScrapeFailure),
}

impl ScrapeResult {
    /// Whether this result is a failure (counts toward the error rate).
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Failure(_))
    }
}

/// One scrape's reported outcome: when it started, how long it took, and how it
/// ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrapeOutcome {
    /// Monotonic start time (nanoseconds) — also the node's new last-scrape time.
    pub started_nanos: u64,
    /// Wall duration of the scrape (nanoseconds) — the latency sample.
    pub duration_nanos: u64,
    /// How the scrape ended.
    pub result: ScrapeResult,
}

impl ScrapeOutcome {
    /// A successful scrape.
    #[must_use]
    pub const fn success(started_nanos: u64, duration_nanos: u64) -> Self {
        Self {
            started_nanos,
            duration_nanos,
            result: ScrapeResult::Success,
        }
    }

    /// A failed scrape with a classified cause.
    #[must_use]
    pub const fn failure(started_nanos: u64, duration_nanos: u64, cause: ScrapeFailure) -> Self {
        Self {
            started_nanos,
            duration_nanos,
            result: ScrapeResult::Failure(cause),
        }
    }
}

/// Scrape scheduling configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrapeConfig {
    /// How often a node should be scraped (`--metric-resolution`), nanoseconds.
    pub resolution_nanos: u64,
    /// Per-scrape timeout budget (nanoseconds).
    pub timeout_nanos: u64,
}

impl ScrapeConfig {
    /// A config with an explicit resolution and timeout (each floored at 1ns to
    /// keep the scheduling arithmetic meaningful).
    #[must_use]
    pub const fn new(resolution_nanos: u64, timeout_nanos: u64) -> Self {
        Self {
            resolution_nanos: if resolution_nanos == 0 {
                1
            } else {
                resolution_nanos
            },
            timeout_nanos: if timeout_nanos == 0 { 1 } else { timeout_nanos },
        }
    }

    /// Whether a scrape of `duration_nanos` overran the timeout budget.
    #[must_use]
    pub const fn is_timed_out(self, duration_nanos: u64) -> bool {
        duration_nanos > self.timeout_nanos
    }
}

impl Default for ScrapeConfig {
    fn default() -> Self {
        Self::new(DEFAULT_RESOLUTION_NANOS, DEFAULT_RESOLUTION_NANOS)
    }
}

/// Per-node scrape bookkeeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeScrapeState {
    /// When the node was last scraped (start time), if ever.
    pub last_started_nanos: Option<u64>,
    /// The duration of the most recent scrape (nanoseconds).
    pub last_duration_nanos: u64,
    /// The result of the most recent scrape, if any.
    pub last_result: Option<ScrapeResult>,
    /// Successive failures since the last success (resets to 0 on success).
    pub consecutive_errors: u32,
    /// Total scrapes recorded for this node.
    pub scrapes: u64,
    /// Total failed scrapes for this node.
    pub errors: u64,
}

/// The scrape scheduler + accountant for a set of nodes.
#[derive(Debug, Clone, Default)]
pub struct Scraper {
    config: ScrapeConfig,
    nodes: BTreeMap<String, NodeScrapeState>,
    total_scrapes: u64,
    total_errors: u64,
    total_duration_nanos: u128,
}

impl Scraper {
    /// A scraper driven by `config`.
    #[must_use]
    pub const fn new(config: ScrapeConfig) -> Self {
        Self {
            config,
            nodes: BTreeMap::new(),
            total_scrapes: 0,
            total_errors: 0,
            total_duration_nanos: 0,
        }
    }

    /// The active configuration.
    #[must_use]
    pub const fn config(&self) -> ScrapeConfig {
        self.config
    }

    /// Whether `node` is due to be scraped at monotonic time `now`: a node never
    /// scraped is always due; otherwise it is due once a full resolution has
    /// elapsed since its last scrape start.
    #[must_use]
    pub fn due(&self, node: &str, now: u64) -> bool {
        self.nodes
            .get(node)
            .and_then(|s| s.last_started_nanos)
            .map_or(true, |last| {
                now.saturating_sub(last) >= self.config.resolution_nanos
            })
    }

    /// Fold a scrape outcome into the per-node and aggregate counters.
    pub fn record(&mut self, node: &str, outcome: ScrapeOutcome) {
        let st = self.nodes.entry(node.to_string()).or_default();
        st.last_started_nanos = Some(outcome.started_nanos);
        st.last_duration_nanos = outcome.duration_nanos;
        st.last_result = Some(outcome.result);
        st.scrapes += 1;
        if outcome.result.is_error() {
            st.errors += 1;
            st.consecutive_errors += 1;
            self.total_errors += 1;
        } else {
            st.consecutive_errors = 0;
        }
        self.total_scrapes += 1;
        self.total_duration_nanos += u128::from(outcome.duration_nanos);
    }

    /// The state of a known node, or `None`.
    #[must_use]
    pub fn node_state(&self, node: &str) -> Option<NodeScrapeState> {
        self.nodes.get(node).copied()
    }

    /// Total scrapes recorded across all nodes.
    #[must_use]
    pub const fn total_scrapes(&self) -> u64 {
        self.total_scrapes
    }

    /// Total failed scrapes across all nodes.
    #[must_use]
    pub const fn total_errors(&self) -> u64 {
        self.total_errors
    }

    /// The scrape error rate in `[0.0, 1.0]` — failures / total. `0.0` with no
    /// scrapes yet.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // counts are small (node-scale); exactness irrelevant for a rate gauge
    pub fn error_rate(&self) -> f64 {
        if self.total_scrapes == 0 {
            return 0.0;
        }
        self.total_errors as f64 / self.total_scrapes as f64
    }

    /// The mean scrape latency across all scrapes (nanoseconds), or `None` if no
    /// scrape has been recorded.
    #[must_use]
    pub fn mean_latency_nanos(&self) -> Option<u64> {
        if self.total_scrapes == 0 {
            return None;
        }
        let mean = self.total_duration_nanos / u128::from(self.total_scrapes);
        Some(u64::try_from(mean).unwrap_or(u64::MAX))
    }

    /// The most recent scrape latency for `node` (nanoseconds), or `None` if the
    /// node has never been scraped.
    #[must_use]
    pub fn last_latency_nanos(&self, node: &str) -> Option<u64> {
        self.nodes
            .get(node)
            .filter(|s| s.last_started_nanos.is_some())
            .map(|s| s.last_duration_nanos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_floors_zero_to_one() {
        let c = ScrapeConfig::new(0, 0);
        assert_eq!(c.resolution_nanos, 1);
        assert_eq!(c.timeout_nanos, 1);
    }

    #[test]
    fn default_config_is_fifteen_seconds() {
        let c = ScrapeConfig::default();
        assert_eq!(c.resolution_nanos, DEFAULT_RESOLUTION_NANOS);
    }

    #[test]
    fn result_error_classification() {
        assert!(ScrapeResult::Failure(ScrapeFailure::Timeout).is_error());
        assert!(!ScrapeResult::Success.is_error());
    }

    #[test]
    fn last_latency_unknown_node_is_none() {
        let s = Scraper::new(ScrapeConfig::default());
        assert!(s.last_latency_nanos("x").is_none());
    }
}
