// SPDX-License-Identifier: Apache-2.0
//! RED-phase test for the **`metrics_server::scraper`** module — the scrape
//! scheduling decision and the per-node latency / error accounting
//! (`pkg/scraper/scraper.go` + the manager's tick loop).
//!
//! metrics-server ticks every `metric-resolution` and scrapes each node's
//! kubelet with a timeout; it records request duration, a success/failure
//! counter, and the last request time. This module is the pure decision spine of
//! that: it answers *which node is due to scrape now* and folds each scrape
//! *outcome* into the counters the observability track exports (scrape latency,
//! scrape error rate). No HTTP — the caller performs the scrape and hands back
//! a [`ScrapeOutcome`].

use cave_home_orchestration::metrics_server::scraper::{
    ScrapeConfig, ScrapeFailure, ScrapeOutcome, Scraper,
};

fn cfg(resolution: u64) -> ScrapeConfig {
    ScrapeConfig::new(resolution, resolution)
}

#[test]
fn never_scraped_node_is_due_immediately() {
    let s = Scraper::new(cfg(1_000));
    assert!(s.due("hub-1", 0));
    assert!(s.due("hub-1", 12_345));
}

#[test]
fn node_is_not_due_again_until_a_resolution_has_passed() {
    let mut s = Scraper::new(cfg(1_000));
    s.record("hub-1", ScrapeOutcome::success(0, 200));
    assert!(!s.due("hub-1", 500)); // half a resolution later
    assert!(!s.due("hub-1", 999));
    assert!(s.due("hub-1", 1_000)); // exactly one resolution → due
    assert!(s.due("hub-1", 5_000));
}

#[test]
fn success_outcome_updates_counters_and_latency() {
    let mut s = Scraper::new(cfg(1_000));
    s.record("hub-1", ScrapeOutcome::success(0, 300));
    assert_eq!(s.total_scrapes(), 1);
    assert_eq!(s.total_errors(), 0);
    assert_eq!(s.error_rate(), 0.0);
    assert_eq!(s.last_latency_nanos("hub-1"), Some(300));
    assert_eq!(s.mean_latency_nanos(), Some(300));
}

#[test]
fn failure_outcome_counts_as_error_and_bumps_consecutive() {
    let mut s = Scraper::new(cfg(1_000));
    s.record(
        "hub-1",
        ScrapeOutcome::failure(0, 800, ScrapeFailure::Timeout),
    );
    s.record(
        "hub-1",
        ScrapeOutcome::failure(1_000, 900, ScrapeFailure::Unreachable),
    );
    assert_eq!(s.total_scrapes(), 2);
    assert_eq!(s.total_errors(), 2);
    assert_eq!(s.error_rate(), 1.0);
    let st = s.node_state("hub-1").expect("known node");
    assert_eq!(st.consecutive_errors, 2);
}

#[test]
fn success_resets_consecutive_errors() {
    let mut s = Scraper::new(cfg(1_000));
    s.record(
        "hub-1",
        ScrapeOutcome::failure(0, 10, ScrapeFailure::Decode),
    );
    s.record("hub-1", ScrapeOutcome::success(1_000, 20));
    let st = s.node_state("hub-1").expect("known node");
    assert_eq!(st.consecutive_errors, 0);
    // One error out of two scrapes → 0.5.
    assert_eq!(s.error_rate(), 0.5);
}

#[test]
fn mean_latency_averages_all_scrapes() {
    let mut s = Scraper::new(cfg(1_000));
    s.record("a", ScrapeOutcome::success(0, 100));
    s.record("b", ScrapeOutcome::success(0, 300));
    assert_eq!(s.mean_latency_nanos(), Some(200));
}

#[test]
fn timeout_predicate_uses_the_configured_budget() {
    let c = cfg(1_000);
    assert!(!c.is_timed_out(999));
    assert!(!c.is_timed_out(1_000));
    assert!(c.is_timed_out(1_001));
}

#[test]
fn empty_scraper_has_zero_rate_and_no_mean() {
    let s = Scraper::new(cfg(1_000));
    assert_eq!(s.total_scrapes(), 0);
    assert_eq!(s.error_rate(), 0.0);
    assert_eq!(s.mean_latency_nanos(), None);
    assert!(s.node_state("anything").is_none());
}
