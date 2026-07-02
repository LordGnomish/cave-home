//! The kubelet `/stats/summary` model and its decode into [`MetricsPoint`]s.
//!
//! Node / pod / container sample extraction, the `decode.go` slice.
//!
//! metrics-server scrapes each node's kubelet Summary API and turns it into the
//! storage unit it keeps: a [`MetricsPoint`] pairing the cumulative CPU counter
//! (`usageCoreNanoSeconds` — total CPU-nanoseconds since the cgroup started)
//! with the memory working-set gauge (`workingSetBytes`) at a sample time. It
//! decodes **one point per node** and **one point per container**; a node or
//! container missing either CPU or memory is skipped, exactly as upstream
//! `decodeNodeStats` / `decodePodStats` skip incomplete stats rather than
//! fabricating a zero.
//!
//! This module models the *decoded* Summary struct and the extraction logic.
//! The JSON deserialization of the raw kubelet response and the HTTPS transport
//! that fetches it are runtime-bound (ADR-004 phase-1b); the caller hands this
//! module an already-decoded [`Summary`].

/// One stored measurement — the unit the rate computation consumes.
///
/// Mirrors upstream `storage.MetricsPoint`: a CPU *counter* (cumulative, so a
/// rate is derived between two points) plus a memory *gauge* (read directly),
/// stamped with the sample time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsPoint {
    /// Monotonic sample time, nanoseconds (the kubelet CPU stat's `time`).
    pub timestamp_nanos: u64,
    /// `CumulativeCpuUsed` — total CPU-nanoseconds consumed (a counter).
    pub cumulative_cpu_nanos: u64,
    /// `MemoryUsage` — the working-set bytes at the sample time (a gauge).
    pub working_set_bytes: u64,
}

/// The kubelet CPU stats block: a cumulative counter and the time it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuStats {
    /// The CPU stat observation time (monotonic nanoseconds).
    pub timestamp_nanos: u64,
    /// `usageCoreNanoSeconds` — cumulative CPU-nanoseconds (the counter).
    pub usage_core_nano_seconds: u64,
}

/// The kubelet memory stats block — the working-set gauge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryStats {
    /// `workingSetBytes` — non-reclaimable memory, the figure metrics-server reports.
    pub working_set_bytes: u64,
}

/// Per-node stats from the Summary API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeStats {
    /// The node's name (the storage key).
    pub node_name: String,
    /// CPU stats, if the kubelet reported them.
    pub cpu: Option<CpuStats>,
    /// Memory stats, if the kubelet reported them.
    pub memory: Option<MemoryStats>,
}

impl NodeStats {
    /// Decode this node's stats into a [`MetricsPoint`], or `None` if either CPU
    /// or memory is missing (upstream skips incomplete node stats).
    #[must_use]
    pub const fn point(&self) -> Option<MetricsPoint> {
        point_from(self.cpu, self.memory)
    }
}

/// The pod identity the Summary API reports (`podRef`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodRef {
    /// Pod name.
    pub name: String,
    /// Pod namespace.
    pub namespace: String,
}

/// Per-container stats inside a pod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerStats {
    /// Container name (the per-pod storage key).
    pub name: String,
    /// CPU stats, if reported.
    pub cpu: Option<CpuStats>,
    /// Memory stats, if reported.
    pub memory: Option<MemoryStats>,
}

/// Per-pod stats: identity plus its containers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodStats {
    /// The pod identity.
    pub pod_ref: PodRef,
    /// The pod's containers.
    pub containers: Vec<ContainerStats>,
}

impl PodStats {
    /// Decode one `(container_name, point)` per container with **complete** CPU
    /// and memory stats, in declaration order. Containers missing either stat
    /// are skipped (`decodePodStats` does not fabricate a zero point).
    #[must_use]
    pub fn container_points(&self) -> Vec<(String, MetricsPoint)> {
        self.containers
            .iter()
            .filter_map(|c| point_from(c.cpu, c.memory).map(|p| (c.name.clone(), p)))
            .collect()
    }
}

/// The decoded kubelet Summary for one node: the node roll-up plus its pods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The node-level stats.
    pub node: NodeStats,
    /// The pods scheduled on this node.
    pub pods: Vec<PodStats>,
}

/// Build a [`MetricsPoint`] from optional CPU + memory blocks: a point requires
/// both. The point's timestamp is the CPU stat's observation time (the counter
/// the rate is derived from).
const fn point_from(cpu: Option<CpuStats>, memory: Option<MemoryStats>) -> Option<MetricsPoint> {
    match (cpu, memory) {
        (Some(c), Some(m)) => Some(MetricsPoint {
            timestamp_nanos: c.timestamp_nanos,
            cumulative_cpu_nanos: c.usage_core_nano_seconds,
            working_set_bytes: m.working_set_bytes,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_from_requires_both_blocks() {
        let c = CpuStats {
            timestamp_nanos: 1,
            usage_core_nano_seconds: 2,
        };
        let m = MemoryStats {
            working_set_bytes: 3,
        };
        assert!(point_from(Some(c), Some(m)).is_some());
        assert!(point_from(Some(c), None).is_none());
        assert!(point_from(None, Some(m)).is_none());
        assert!(point_from(None, None).is_none());
    }

    #[test]
    fn container_points_preserve_order() {
        let pod = PodStats {
            pod_ref: PodRef {
                name: "p".into(),
                namespace: "n".into(),
            },
            containers: vec![
                ContainerStats {
                    name: "a".into(),
                    cpu: Some(CpuStats {
                        timestamp_nanos: 1,
                        usage_core_nano_seconds: 1,
                    }),
                    memory: Some(MemoryStats {
                        working_set_bytes: 1,
                    }),
                },
                ContainerStats {
                    name: "b".into(),
                    cpu: Some(CpuStats {
                        timestamp_nanos: 1,
                        usage_core_nano_seconds: 2,
                    }),
                    memory: Some(MemoryStats {
                        working_set_bytes: 2,
                    }),
                },
            ],
        };
        let names: Vec<_> = pod.container_points().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["a", "b"]);
    }
}
