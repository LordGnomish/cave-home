// SPDX-License-Identifier: Apache-2.0
//! RED-phase test for the **`metrics_server::observability`** module — the
//! Prometheus metric-name constants and the snapshot the scrape pipeline
//! exports (scrape latency, scrape error rate, storage memory footprint).
//!
//! metrics-server self-instruments: scrape duration, a scrape success/failure
//! counter, and storage size. This module names those series and folds a live
//! [`Scraper`] + [`Storage`] into a single [`MetricsSnapshot`] the observability
//! track (`observability/panels/metrics-server.json`) renders.

use cave_home_orchestration::metrics_server::observability::{metric_names, MetricsSnapshot};
use cave_home_orchestration::metrics_server::scraper::{ScrapeConfig, ScrapeOutcome, ScrapeFailure, Scraper};
use cave_home_orchestration::metrics_server::store::Storage;
use cave_home_orchestration::metrics_server::summary::MetricsPoint;

fn point(ts: u64, cpu: u64, mem: u64) -> MetricsPoint {
    MetricsPoint { timestamp_nanos: ts, cumulative_cpu_nanos: cpu, working_set_bytes: mem }
}

#[test]
fn metric_names_are_namespaced_and_unique() {
    let names = metric_names();
    assert!(names.iter().all(|n| n.starts_with("cave_home_metrics_server_")));
    let mut sorted = names.to_vec();
    sorted.sort_unstable();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(sorted.len(), before, "metric names must be unique");
    // The three series the task calls out are present.
    assert!(names.contains(&"cave_home_metrics_server_scrape_duration_seconds"));
    assert!(names.contains(&"cave_home_metrics_server_scrape_errors_total"));
    assert!(names.contains(&"cave_home_metrics_server_storage_memory_bytes"));
}

#[test]
fn snapshot_folds_scraper_and_storage() {
    let mut scraper = Scraper::new(ScrapeConfig::new(1_000, 1_000));
    scraper.record("hub-1", ScrapeOutcome::success(0, 200));
    scraper.record("hub-1", ScrapeOutcome::failure(1_000, 400, ScrapeFailure::Timeout));

    let mut store = Storage::new();
    store.store_node("hub-1", point(0, 0, 1));
    store.store_node("hub-1", point(1, 1, 1));
    store.store_container("ns", "p", "c", point(0, 0, 1));

    let snap = MetricsSnapshot::collect(&scraper, &store);
    assert_eq!(snap.scrapes_total, 2);
    assert_eq!(snap.scrape_errors_total, 1);
    assert_eq!(snap.scrape_error_rate, 0.5);
    assert_eq!(snap.mean_scrape_latency_nanos, 300);
    // 3 points stored (2 node + 1 container).
    assert_eq!(snap.storage_points, 3);
    // Footprint scales with the point count and is non-zero when points exist.
    assert!(snap.storage_memory_bytes >= 3);
    assert_eq!(snap.node_count, 1);
    assert_eq!(snap.pod_count, 1);
}

#[test]
fn empty_snapshot_is_all_zero() {
    let scraper = Scraper::new(ScrapeConfig::default());
    let store = Storage::new();
    let snap = MetricsSnapshot::collect(&scraper, &store);
    assert_eq!(snap.scrapes_total, 0);
    assert_eq!(snap.scrape_error_rate, 0.0);
    assert_eq!(snap.mean_scrape_latency_nanos, 0);
    assert_eq!(snap.storage_points, 0);
    assert_eq!(snap.storage_memory_bytes, 0);
}
