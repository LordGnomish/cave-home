// SPDX-License-Identifier: Apache-2.0
//! RED-phase test for the **`metrics_server::summary`** module — the kubelet
//! `/stats/summary` data model and the node / pod / container sample extraction
//! (`pkg/scraper/client/summary/decode.go`).
//!
//! metrics-server scrapes each node's kubelet Summary API and *decodes* it into
//! `storage.MetricsPoint`s: one per node and one per container. A point pairs
//! the cumulative CPU counter (`usageCoreNanoSeconds`, total CPU-nanoseconds)
//! with the memory working-set gauge (`workingSetBytes`) at the sample time. A
//! node or container missing either CPU or memory is skipped (no point), exactly
//! as upstream `decodeNodeStats` / `decodePodStats` skip incomplete stats.
//!
//! This drives the decode logic only — the JSON deserialization + the kubelet
//! HTTPS transport are runtime-bound and stay deferred; the caller supplies the
//! already-decoded [`Summary`] struct.

use cave_home_orchestration::metrics_server::summary::{
    ContainerStats, CpuStats, MemoryStats, MetricsPoint, NodeStats, PodRef, PodStats, Summary,
};

fn cpu(ts: u64, cumulative: u64) -> CpuStats {
    CpuStats { timestamp_nanos: ts, usage_core_nano_seconds: cumulative }
}

fn mem(ws: u64) -> MemoryStats {
    MemoryStats { working_set_bytes: ws }
}

#[test]
fn node_point_pairs_cpu_counter_and_memory_gauge() {
    let node = NodeStats {
        node_name: "hub-1".into(),
        cpu: Some(cpu(1_000, 5_000_000_000)),
        memory: Some(mem(256 * 1024 * 1024)),
    };
    let p = node.point().expect("complete node stats yield a point");
    assert_eq!(
        p,
        MetricsPoint {
            timestamp_nanos: 1_000,
            cumulative_cpu_nanos: 5_000_000_000,
            working_set_bytes: 256 * 1024 * 1024,
        }
    );
}

#[test]
fn node_missing_cpu_or_memory_yields_no_point() {
    let no_cpu = NodeStats { node_name: "n".into(), cpu: None, memory: Some(mem(1)) };
    assert!(no_cpu.point().is_none());
    let no_mem = NodeStats { node_name: "n".into(), cpu: Some(cpu(1, 1)), memory: None };
    assert!(no_mem.point().is_none());
}

#[test]
fn pod_decodes_one_point_per_container() {
    let pod = PodStats {
        pod_ref: PodRef { name: "web".into(), namespace: "apps".into() },
        containers: vec![
            ContainerStats {
                name: "nginx".into(),
                cpu: Some(cpu(2_000, 1_000_000_000)),
                memory: Some(mem(32 * 1024 * 1024)),
            },
            ContainerStats {
                name: "sidecar".into(),
                cpu: Some(cpu(2_000, 500_000_000)),
                memory: Some(mem(16 * 1024 * 1024)),
            },
        ],
    };
    let points = pod.container_points();
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].0, "nginx");
    assert_eq!(points[0].1.cumulative_cpu_nanos, 1_000_000_000);
    assert_eq!(points[0].1.working_set_bytes, 32 * 1024 * 1024);
    assert_eq!(points[1].0, "sidecar");
    assert_eq!(points[1].1.working_set_bytes, 16 * 1024 * 1024);
}

#[test]
fn pod_skips_containers_with_incomplete_stats() {
    let pod = PodStats {
        pod_ref: PodRef { name: "p".into(), namespace: "default".into() },
        containers: vec![
            ContainerStats { name: "ok".into(), cpu: Some(cpu(1, 10)), memory: Some(mem(20)) },
            ContainerStats { name: "no-mem".into(), cpu: Some(cpu(1, 10)), memory: None },
            ContainerStats { name: "no-cpu".into(), cpu: None, memory: Some(mem(20)) },
        ],
    };
    let points = pod.container_points();
    assert_eq!(points.len(), 1, "only the complete container is decoded");
    assert_eq!(points[0].0, "ok");
}

#[test]
fn summary_holds_node_and_pods() {
    let s = Summary {
        node: NodeStats {
            node_name: "hub-1".into(),
            cpu: Some(cpu(9, 9)),
            memory: Some(mem(9)),
        },
        pods: vec![PodStats {
            pod_ref: PodRef { name: "p".into(), namespace: "ns".into() },
            containers: vec![],
        }],
    };
    assert_eq!(s.node.node_name, "hub-1");
    assert_eq!(s.pods.len(), 1);
    // A pod with no decodable containers contributes no points.
    assert!(s.pods[0].container_points().is_empty());
}
