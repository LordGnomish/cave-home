//! Prometheus metric-name constants and the exported pipeline snapshot.
//!
//! Scrape latency, scrape error rate, and storage memory footprint.
//!
//! metrics-server self-instruments — scrape duration, a scrape success/failure
//! counter, and storage size. This module names those series (the cave-home
//! `cave_home_metrics_server_*` namespace) and folds a live [`Scraper`] +
//! [`Storage`] into one [`MetricsSnapshot`] the observability track
//! (`observability/panels/metrics-server.json`) renders. It is the source of
//! truth for the metric names the panel JSON references.

use super::scraper::Scraper;
use super::store::Storage;
use super::summary::MetricsPoint;

/// `cave_home_metrics_server_scrape_duration_seconds` — kubelet scrape latency.
pub const SCRAPE_DURATION_SECONDS: &str = "cave_home_metrics_server_scrape_duration_seconds";

/// `cave_home_metrics_server_scrapes_total` — total scrapes attempted.
pub const SCRAPES_TOTAL: &str = "cave_home_metrics_server_scrapes_total";

/// `cave_home_metrics_server_scrape_errors_total` — failed scrapes.
pub const SCRAPE_ERRORS_TOTAL: &str = "cave_home_metrics_server_scrape_errors_total";

/// `cave_home_metrics_server_storage_points` — points retained across all rings.
pub const STORAGE_POINTS: &str = "cave_home_metrics_server_storage_points";

/// `cave_home_metrics_server_storage_memory_bytes` — storage memory footprint.
pub const STORAGE_MEMORY_BYTES: &str = "cave_home_metrics_server_storage_memory_bytes";

/// Every exported metric name, for discovery / panel-consistency checks.
#[must_use]
pub fn metric_names() -> Vec<&'static str> {
    vec![
        SCRAPE_DURATION_SECONDS,
        SCRAPES_TOTAL,
        SCRAPE_ERRORS_TOTAL,
        STORAGE_POINTS,
        STORAGE_MEMORY_BYTES,
    ]
}

/// The in-memory size of one stored [`MetricsPoint`] — the per-point cost the
/// storage memory footprint is estimated from.
const POINT_SIZE_BYTES: usize = core::mem::size_of::<MetricsPoint>();

/// A point-in-time read of the pipeline's self-instrumentation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricsSnapshot {
    /// Total scrapes attempted.
    pub scrapes_total: u64,
    /// Failed scrapes.
    pub scrape_errors_total: u64,
    /// Failures / total, in `[0.0, 1.0]`.
    pub scrape_error_rate: f64,
    /// Mean scrape latency (nanoseconds); `0` when nothing has been scraped.
    pub mean_scrape_latency_nanos: u64,
    /// Points retained across every ring.
    pub storage_points: usize,
    /// Estimated storage memory footprint (bytes) = points × point size.
    pub storage_memory_bytes: usize,
    /// Nodes tracked.
    pub node_count: usize,
    /// Pods tracked.
    pub pod_count: usize,
}

impl MetricsSnapshot {
    /// Fold the scraper's counters and the store's size into one snapshot.
    #[must_use]
    pub fn collect(scraper: &Scraper, storage: &Storage) -> Self {
        let storage_points = storage.points_stored();
        Self {
            scrapes_total: scraper.total_scrapes(),
            scrape_errors_total: scraper.total_errors(),
            scrape_error_rate: scraper.error_rate(),
            mean_scrape_latency_nanos: scraper.mean_latency_nanos().unwrap_or(0),
            storage_points,
            storage_memory_bytes: storage_points.saturating_mul(POINT_SIZE_BYTES),
            node_count: storage.node_count(),
            pod_count: storage.pod_count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_size_is_three_u64s() {
        // MetricsPoint is three u64 fields; the footprint estimate rests on this.
        assert_eq!(POINT_SIZE_BYTES, 24);
    }

    #[test]
    fn all_names_are_in_the_namespace() {
        assert!(
            metric_names()
                .iter()
                .all(|n| n.starts_with("cave_home_metrics_server_"))
        );
    }
}
