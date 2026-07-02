// SPDX-License-Identifier: Apache-2.0
//! RED-phase test for the **`metrics_server::store`** module — the in-memory
//! ring-buffer point storage and the cumulative-counter → CPU-rate computation
//! (`pkg/storage/point.go` `resourceUsage` + the node/pod stores).
//!
//! metrics-server keeps the most recent samples per object so it can derive a
//! CPU usage *rate* from the cumulative `usageCoreNanoSeconds` counter:
//! `nanocores = Δcpu_ns · 1e9 / Δwall_ns`. Memory is a gauge, read from the
//! latest point. The store rejects a counter that went **backwards** (a restart
//! / cgroup reset) and a **non-increasing timestamp** (zero window) rather than
//! emitting a bogus rate. Fewer than two points means no rate yet.

use cave_home_orchestration::metrics_server::store::{PointRing, RateError, Storage, Usage};
use cave_home_orchestration::metrics_server::summary::MetricsPoint;

fn point(ts: u64, cpu: u64, mem: u64) -> MetricsPoint {
    MetricsPoint {
        timestamp_nanos: ts,
        cumulative_cpu_nanos: cpu,
        working_set_bytes: mem,
    }
}

#[test]
fn rate_from_two_points_is_nanocores_over_window() {
    // 0.25 core-second consumed over a 1s window → 0.25 cores = 250m.
    let prev = point(0, 0, 0);
    let last = point(1_000_000_000, 250_000_000, 64 * 1024 * 1024);
    let u: Usage = Usage::between(prev, last).expect("monotonic, no reset");
    assert_eq!(u.usage.cpu.to_cpu_string(), "250m");
    // Memory is the latest gauge, not a rate.
    assert_eq!(u.usage.memory.raw(), 64 * 1024 * 1024);
    assert_eq!(u.window_nanos, 1_000_000_000);
    assert_eq!(u.timestamp_nanos, 1_000_000_000);
}

#[test]
fn full_core_for_two_seconds_is_one_thousand_milli() {
    // 1 core for 2s = 2e9 CPU-ns over a 2s window → 1 core = "1".
    let prev = point(1_000_000_000, 5_000_000_000, 10);
    let last = point(3_000_000_000, 7_000_000_000, 20);
    let u = Usage::between(prev, last).expect("ok");
    assert_eq!(u.usage.cpu.to_cpu_string(), "1");
    assert_eq!(u.usage.cpu.milli_cpu(), 1000);
}

#[test]
fn counter_decrease_is_a_reset_error() {
    let prev = point(0, 9_000_000_000, 1);
    let last = point(1_000_000_000, 1_000_000_000, 1); // counter went backwards
    assert_eq!(Usage::between(prev, last), Err(RateError::CounterReset));
}

#[test]
fn non_increasing_timestamp_is_a_zero_window_error() {
    let p = point(5, 1, 1);
    assert_eq!(Usage::between(p, p), Err(RateError::NonMonotonicTime));
    let earlier = point(4, 2, 1);
    assert_eq!(Usage::between(p, earlier), Err(RateError::NonMonotonicTime));
}

#[test]
fn ring_keeps_last_two_and_derives_usage() {
    let mut ring = PointRing::with_capacity(2);
    assert_eq!(ring.usage(), Err(RateError::InsufficientData));
    ring.push(point(0, 0, 1));
    assert_eq!(ring.usage(), Err(RateError::InsufficientData));
    ring.push(point(1_000_000_000, 250_000_000, 2));
    // A third push evicts the oldest; the rate uses the two most recent.
    ring.push(point(2_000_000_000, 750_000_000, 3));
    assert_eq!(ring.len(), 2);
    let u = ring.usage().expect("two points present");
    // Δcpu = 500_000_000 over 1s → 500m.
    assert_eq!(u.usage.cpu.to_cpu_string(), "500m");
    assert_eq!(u.usage.memory.raw(), 3);
}

#[test]
fn storage_tracks_nodes_and_pod_containers() {
    let mut store = Storage::new();
    store.store_node("hub-1", point(0, 0, 100));
    store.store_node("hub-1", point(1_000_000_000, 1_000_000_000, 200));
    let nu = store
        .node_usage("hub-1")
        .expect("node present")
        .expect("rate ok");
    assert_eq!(nu.usage.cpu.to_cpu_string(), "1");
    assert_eq!(nu.usage.memory.raw(), 200);
    assert!(store.node_usage("absent").is_none());

    // Two containers of one pod, two samples each.
    store.store_container("apps", "web", "nginx", point(0, 0, 10));
    store.store_container(
        "apps",
        "web",
        "nginx",
        point(1_000_000_000, 100_000_000, 20),
    );
    store.store_container("apps", "web", "log", point(0, 0, 30));
    store.store_container("apps", "web", "log", point(1_000_000_000, 150_000_000, 40));
    let usages = store.pod_container_usages("apps", "web");
    assert_eq!(usages.len(), 2);
    // Sorted by container name: log, nginx.
    assert_eq!(usages[0].0, "log");
    assert_eq!(usages[0].1.usage.cpu.to_cpu_string(), "150m");
    assert_eq!(usages[1].0, "nginx");
    assert_eq!(usages[1].1.usage.cpu.to_cpu_string(), "100m");
}

#[test]
fn points_stored_counts_for_memory_footprint() {
    let mut store = Storage::new();
    assert_eq!(store.points_stored(), 0);
    store.store_node("a", point(0, 0, 1));
    store.store_node("a", point(1, 1, 1));
    store.store_container("ns", "p", "c", point(0, 0, 1));
    // node "a" holds 2 points, container c holds 1 → 3 total.
    assert_eq!(store.points_stored(), 3);
    assert_eq!(store.node_count(), 1);
    assert_eq!(store.pod_count(), 1);
}
