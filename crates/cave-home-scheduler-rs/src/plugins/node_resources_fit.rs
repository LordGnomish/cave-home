// SPDX-License-Identifier: Apache-2.0
//! `NodeResourcesFit` — reject nodes whose allocatable - requested < pod request.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/noderesources/fit.go::Filter

use crate::cache::NodeInfo;
use crate::framework::{CycleState, FilterPlugin, Status};
use crate::types::{Pod, ResourceName};

pub struct NodeResourcesFit;

impl FilterPlugin for NodeResourcesFit {
    fn name(&self) -> &'static str {
        "NodeResourcesFit"
    }

    fn filter(&self, _state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> Status {
        for resource in [ResourceName::Cpu, ResourceName::Memory] {
            let request = pod.sum_requests(resource).value();
            if request == 0 {
                continue;
            }
            let allocatable = node.allocatable(resource).value();
            let already = node.requested(resource);
            let free = allocatable.saturating_sub(already);
            if request > free {
                return Status::unschedulable(
                    self.name(),
                    format!(
                        "Insufficient {:?}: requested {request}, free {free}",
                        resource
                    ),
                );
            }
        }
        Status::success()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Container, Node, Pod, Quantity, ResourceName};

    fn node(cpu_m: i64, mem_b: i64) -> Node {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
        n.status
            .allocatable
            .insert(ResourceName::Memory, Quantity::bytes(mem_b));
        n
    }

    fn pod(cpu_m: i64, mem_b: i64) -> Pod {
        let mut p = Pod::default();
        let mut c = Container::default();
        if cpu_m > 0 {
            c.resources
                .requests
                .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
        }
        if mem_b > 0 {
            c.resources
                .requests
                .insert(ResourceName::Memory, Quantity::bytes(mem_b));
        }
        p.spec.containers.push(c);
        p
    }

    #[test]
    fn fits_when_request_below_allocatable() {
        let info = NodeInfo::new(node(2000, 4096));
        let p = pod(500, 1024);
        let mut s = CycleState::new();
        assert!(NodeResourcesFit.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn does_not_fit_when_cpu_exceeds_free() {
        let info = NodeInfo::new(node(500, 4096));
        let p = pod(600, 0);
        let mut s = CycleState::new();
        let st = NodeResourcesFit.filter(&mut s, &p, &info);
        assert!(!st.is_success());
    }

    #[test]
    fn accounts_for_already_requested_pods() {
        let mut info = NodeInfo::new(node(1000, 4096));
        let prior = pod(800, 0);
        info.add_pod(prior);
        let p = pod(300, 0);
        let mut s = CycleState::new();
        let st = NodeResourcesFit.filter(&mut s, &p, &info);
        assert!(!st.is_success());
    }

    #[test]
    fn zero_request_pod_fits_anywhere() {
        let info = NodeInfo::new(node(0, 0));
        let p = Pod::default();
        let mut s = CycleState::new();
        assert!(NodeResourcesFit.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn prefilter_precomputes_pod_requests_into_state() {
        use crate::framework::PreFilterPlugin;
        let p = pod(500, 1024);
        let mut s = CycleState::new();
        let (res, status) = NodeResourcesFit.pre_filter(&mut s, &p);
        assert!(status.is_success());
        // NodeResourcesFit does not restrict the node set.
        assert!(res.is_none_or(|r| r.node_names.is_none()));
        // The aggregated request is cached for the Filter phase to reuse.
        let cached = s
            .read::<PreFilterFitState>(PRE_FILTER_FIT_KEY)
            .expect("pre-filter state recorded");
        assert_eq!(cached.cpu, 500);
        assert_eq!(cached.memory, 1024);
    }

    #[test]
    fn filter_uses_precomputed_prefilter_state_when_present() {
        use crate::framework::PreFilterPlugin;
        let info = NodeInfo::new(node(1000, 4096));
        let p = pod(600, 0);
        let mut s = CycleState::new();
        // Precompute once...
        NodeResourcesFit.pre_filter(&mut s, &p);
        // ...then Filter reuses it and still fits.
        assert!(NodeResourcesFit.filter(&mut s, &p, &info).is_success());
    }
}
