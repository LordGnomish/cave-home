// SPDX-License-Identifier: Apache-2.0
//! RED-phase test for the **Portal Metrics page** — the developer-only live
//! node / workload CPU + memory dashboard backed by the in-process
//! metrics_server pipeline.
//!
//! The cluster resource metrics (`kubectl top`-class data) are power-user
//! content, so the page is **developer-only** (Charter §6.3): it is structurally
//! absent from resident / mobile output. This drives the page view-model and the
//! `Card::ClusterMetrics` slot; the live usage is fed in by the Portal backend.

use cave_home_portal::card::Card;
use cave_home_portal::metrics::{MetricsPage, NodeUsageTile, WorkloadUsageTile};

#[test]
fn cluster_metrics_card_is_developer_only() {
    assert!(Card::ClusterMetrics.is_developer_only());
}

#[test]
fn node_tile_formats_friendly_labels() {
    let t = NodeUsageTile::new("Hub", 250, Some(12), 128, Some(25));
    assert_eq!(t.cpu_label(), "250m");
    assert_eq!(t.memory_label(), "128Mi");
    assert_eq!(t.cpu_percent_label(), "12%");
    assert_eq!(t.memory_percent_label(), "25%");
}

#[test]
fn node_tile_unknown_percent_shows_dash() {
    let t = NodeUsageTile::new("Hub", 250, None, 128, None);
    assert_eq!(t.cpu_percent_label(), "—");
    assert_eq!(t.memory_percent_label(), "—");
}

#[test]
fn workload_tile_formats_usage() {
    let w = WorkloadUsageTile::new("Cameras", 500, 256);
    assert_eq!(w.name, "Cameras");
    assert_eq!(w.cpu_label(), "500m");
    assert_eq!(w.memory_label(), "256Mi");
}

#[test]
fn page_summarises_counts() {
    let page = MetricsPage::new(
        vec![
            NodeUsageTile::new("Hub", 250, Some(12), 128, Some(25)),
            NodeUsageTile::new("Backup hub", 100, Some(5), 64, Some(10)),
        ],
        vec![WorkloadUsageTile::new("Cameras", 500, 256)],
    );
    assert_eq!(page.node_count(), 2);
    assert_eq!(page.workload_count(), 1);
}

#[test]
fn page_renders_as_a_developer_only_view() {
    let page = MetricsPage::new(
        vec![NodeUsageTile::new("Hub", 250, Some(12), 128, Some(25))],
        vec![],
    );
    let view = page.into_view();
    // The whole tab is power-user-only and structurally carries the metrics card.
    assert!(view.developer_only);
    assert!(view.cards.contains(&Card::ClusterMetrics));
}
