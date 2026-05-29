// SPDX-License-Identifier: Apache-2.0
//! Node resource accounting + pod admission (the "does it fit?" check).
//!
//! Behavioural reimplementation of the documented kubelet predicate admit
//! handler (`pkg/kubelet/lifecycle/predicate.go` + the `NodeResourcesFit`
//! contract it reuses): the kubelet, before running a pod the scheduler placed
//! on it, re-checks that the pod's requests still fit within the node's
//! allocatable resources after accounting for already-admitted pods.
//!
//! `Allocatable = Capacity - reserved`. A new pod is admitted iff
//! `sum(already-admitted requests) + new pod requests <= allocatable` on
//! **every** resource axis.
//!
//! Pure, `std`-only.

use crate::resources::{sum_requests, ResourceList, ResourceRequirements};

/// Per-node resource accounting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NodeResources {
    /// Allocatable resources (capacity minus system/kube reserved).
    pub allocatable: ResourceList,
    /// Sum of requests of pods already admitted to this node.
    pub requested: ResourceList,
}

impl NodeResources {
    /// New node with the given allocatable and zero requested.
    #[must_use]
    pub const fn new(allocatable: ResourceList) -> Self {
        Self {
            allocatable,
            requested: ResourceList::new(0, 0),
        }
    }

    /// Resources still free for new pods (saturating; never negative).
    #[must_use]
    pub const fn available(&self) -> ResourceList {
        ResourceList {
            cpu_milli: self.allocatable.cpu_milli.saturating_sub(self.requested.cpu_milli),
            memory_bytes: self
                .allocatable
                .memory_bytes
                .saturating_sub(self.requested.memory_bytes),
        }
    }

    /// Account a pod's containers as admitted, growing `requested`.
    pub fn admit(&mut self, containers: &[ResourceRequirements]) {
        self.requested = self.requested.add(sum_requests(containers));
    }
}

/// The result of an admission check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionResult {
    /// The pod fits and may run.
    Admit,
    /// The pod does not fit (insufficient resources).
    Reject(InsufficientResource),
}

/// Which axis (and by how much) caused a rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InsufficientResource {
    /// Not enough CPU; carries the shortfall in milli-cores.
    Cpu { short_by_milli: u64 },
    /// Not enough memory; carries the shortfall in bytes.
    Memory { short_by_bytes: u64 },
}

/// Decide whether a pod (given its containers' requirements) fits on `node`.
///
/// CPU is checked first, then memory (matching the documented predicate order),
/// so the reported [`InsufficientResource`] is deterministic.
#[must_use]
pub fn admit_pod(node: &NodeResources, containers: &[ResourceRequirements]) -> AdmissionResult {
    let need = sum_requests(containers);
    let free = node.available();

    if need.cpu_milli > free.cpu_milli {
        return AdmissionResult::Reject(InsufficientResource::Cpu {
            short_by_milli: need.cpu_milli - free.cpu_milli,
        });
    }
    if need.memory_bytes > free.memory_bytes {
        return AdmissionResult::Reject(InsufficientResource::Memory {
            short_by_bytes: need.memory_bytes - free.memory_bytes,
        });
    }
    AdmissionResult::Admit
}

/// Convenience boolean form of [`admit_pod`].
#[must_use]
pub fn pod_fits(node: &NodeResources, containers: &[ResourceRequirements]) -> bool {
    matches!(admit_pod(node, containers), AdmissionResult::Admit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(cpu: u64, mem: u64) -> ResourceRequirements {
        ResourceRequirements {
            cpu_request_milli: Some(cpu),
            memory_request_bytes: Some(mem),
            ..Default::default()
        }
    }

    fn node(cpu: u64, mem: u64) -> NodeResources {
        NodeResources::new(ResourceList::new(cpu, mem))
    }

    #[test]
    fn empty_node_available_equals_allocatable() {
        let n = node(2000, 4_000_000);
        assert_eq!(n.available(), ResourceList::new(2000, 4_000_000));
    }

    #[test]
    fn pod_fits_on_empty_node() {
        let n = node(2000, 4_000_000);
        assert_eq!(admit_pod(&n, &[req(1000, 2_000_000)]), AdmissionResult::Admit);
        assert!(pod_fits(&n, &[req(1000, 2_000_000)]));
    }

    #[test]
    fn exact_fit_is_admitted() {
        let n = node(1000, 1000);
        assert_eq!(admit_pod(&n, &[req(1000, 1000)]), AdmissionResult::Admit);
    }

    #[test]
    fn zero_request_pod_always_fits() {
        let n = node(0, 0);
        assert!(pod_fits(&n, &[ResourceRequirements::default()]));
    }

    #[test]
    fn cpu_overcommit_is_rejected() {
        let n = node(1000, 4_000_000);
        assert_eq!(
            admit_pod(&n, &[req(1500, 1000)]),
            AdmissionResult::Reject(InsufficientResource::Cpu { short_by_milli: 500 })
        );
    }

    #[test]
    fn memory_overcommit_is_rejected() {
        let n = node(2000, 1_000_000);
        assert_eq!(
            admit_pod(&n, &[req(500, 1_500_000)]),
            AdmissionResult::Reject(InsufficientResource::Memory {
                short_by_bytes: 500_000
            })
        );
    }

    #[test]
    fn cpu_checked_before_memory() {
        // Both axes short: CPU shortfall must be reported (deterministic order).
        let n = node(100, 100);
        assert_eq!(
            admit_pod(&n, &[req(200, 200)]),
            AdmissionResult::Reject(InsufficientResource::Cpu { short_by_milli: 100 })
        );
    }

    #[test]
    fn admitting_pods_consumes_capacity() {
        let mut n = node(2000, 4_000_000);
        n.admit(&[req(1500, 3_000_000)]);
        assert_eq!(n.available(), ResourceList::new(500, 1_000_000));
        // A second pod that no longer fits is rejected.
        assert!(!pod_fits(&n, &[req(600, 0)]));
        // But one that fits the remainder is admitted.
        assert!(pod_fits(&n, &[req(500, 1_000_000)]));
    }

    #[test]
    fn multi_container_pod_sums_requests() {
        let n = node(1000, 0);
        // Two containers each 600m = 1200m > 1000m.
        assert!(!pod_fits(&n, &[req(600, 0), req(600, 0)]));
        assert!(pod_fits(&n, &[req(400, 0), req(600, 0)]));
    }

    #[test]
    fn available_never_goes_negative() {
        let mut n = node(1000, 1000);
        n.admit(&[req(1000, 1000)]);
        n.admit(&[req(1000, 1000)]); // over-admit (e.g. raced); still saturating
        assert_eq!(n.available(), ResourceList::new(0, 0));
    }
}
