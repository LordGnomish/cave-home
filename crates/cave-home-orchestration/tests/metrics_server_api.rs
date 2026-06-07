// SPDX-License-Identifier: Apache-2.0
//! RED-phase test for the **`metrics_server::api`** module — the
//! `metrics.k8s.io/v1beta1` resource-metrics objects and the aggregated
//! `APIService` registration (`pkg/api` + the aggregation-layer wiring).
//!
//! metrics-server serves `NodeMetrics` and `PodMetrics` through the Kubernetes
//! aggregation layer: it registers an `APIService` (`v1beta1.metrics.k8s.io`)
//! that points the apiserver at the metrics service, then answers GET/LIST with
//! objects built from its storage. A `PodMetrics` carries per-container usage;
//! the pod total is the sum of its containers. This module builds those objects
//! from [`store::Usage`] and describes the `APIService` to register.

use cave_home_orchestration::metrics_server::api::{
    ApiService, ContainerMetrics, NodeMetrics, PodMetrics, GROUP, VERSION,
};
use cave_home_orchestration::metrics_server::quantity::{Quantity, ResourceList};
use cave_home_orchestration::metrics_server::store::{Storage, Usage};
use cave_home_orchestration::metrics_server::summary::MetricsPoint;

fn usage(ts: u64, window: u64, cpu_nano: u64, mem: u64) -> Usage {
    Usage {
        timestamp_nanos: ts,
        window_nanos: window,
        usage: ResourceList::new(Quantity::from_cpu_nanocores(cpu_nano), Quantity::from_bytes(mem)),
    }
}

fn point(ts: u64, cpu: u64, mem: u64) -> MetricsPoint {
    MetricsPoint { timestamp_nanos: ts, cumulative_cpu_nanos: cpu, working_set_bytes: mem }
}

#[test]
fn node_metrics_from_usage_carries_typemeta_and_window() {
    let nm = NodeMetrics::from_usage("hub-1", &usage(5_000, 1_000, 250_000_000, 64 * 1024 * 1024));
    assert_eq!(nm.name, "hub-1");
    assert_eq!(nm.timestamp_nanos, 5_000);
    assert_eq!(nm.window_nanos, 1_000);
    assert_eq!(nm.usage.cpu.to_cpu_string(), "250m");
    assert_eq!(NodeMetrics::KIND, "NodeMetrics");
    assert_eq!(nm.api_version(), "metrics.k8s.io/v1beta1");
}

#[test]
fn pod_metrics_total_sums_containers() {
    let containers = vec![
        ("nginx".to_string(), usage(10, 1_000, 100_000_000, 32 * 1024 * 1024)),
        ("log".to_string(), usage(20, 1_000, 150_000_000, 96 * 1024 * 1024)),
    ];
    let pm = PodMetrics::from_container_usages("apps", "web", &containers);
    assert_eq!(pm.namespace, "apps");
    assert_eq!(pm.name, "web");
    assert_eq!(pm.containers.len(), 2);
    // The pod window/timestamp is the minimum across its containers.
    assert_eq!(pm.timestamp_nanos, 10);
    assert_eq!(pm.window_nanos, 1_000);
    // Total = 100m + 150m = 250m CPU; 32Mi + 96Mi = 128Mi memory.
    let total = pm.total();
    assert_eq!(total.cpu.to_cpu_string(), "250m");
    assert_eq!(total.memory.to_mem_string(), "128Mi");
    assert_eq!(PodMetrics::KIND, "PodMetrics");
}

#[test]
fn container_metrics_pairs_name_and_usage() {
    let c = ContainerMetrics::new("c1", ResourceList::zero());
    assert_eq!(c.name, "c1");
    assert_eq!(c.usage.cpu.raw(), 0);
}

#[test]
fn apiservice_describes_the_aggregated_registration() {
    let svc = ApiService::metrics_v1beta1();
    assert_eq!(svc.name, "v1beta1.metrics.k8s.io");
    assert_eq!(svc.group, GROUP);
    assert_eq!(svc.version, VERSION);
    assert_eq!(GROUP, "metrics.k8s.io");
    assert_eq!(VERSION, "v1beta1");
    // The aggregation layer needs a deterministic priority pair.
    assert_eq!(svc.group_priority_minimum, 100);
    assert_eq!(svc.version_priority, 100);
}

#[test]
fn lists_metrics_from_storage_skipping_unrateable() {
    let mut store = Storage::new();
    // hub-1 has two samples → rateable; hub-2 has one → skipped.
    store.store_node("hub-1", point(0, 0, 100));
    store.store_node("hub-1", point(1_000_000_000, 1_000_000_000, 200));
    store.store_node("hub-2", point(0, 0, 50));

    let nodes = NodeMetrics::list_from_storage(&store);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name, "hub-1");
    assert_eq!(nodes[0].usage.cpu.to_cpu_string(), "1");

    store.store_container("apps", "web", "nginx", point(0, 0, 10));
    store.store_container("apps", "web", "nginx", point(1_000_000_000, 100_000_000, 20));
    let pods = PodMetrics::list_from_storage(&store);
    assert_eq!(pods.len(), 1);
    assert_eq!(pods[0].name, "web");
    assert_eq!(pods[0].total().cpu.to_cpu_string(), "100m");
}
