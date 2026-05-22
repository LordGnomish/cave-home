// SPDX-License-Identifier: Apache-2.0
//! `NodePorts` — reject nodes that already use a host port the pod claims.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/nodeports/node_ports.go

use crate::cache::node_info::HostPortUse;
use crate::cache::NodeInfo;
use crate::framework::{CycleState, FilterPlugin, Status};
use crate::types::Pod;

pub struct NodePorts;

impl NodePorts {
    /// Upstream: `nodeports.fits` — replays the host port aggregation.
    fn fits(pod: &Pod, node: &NodeInfo) -> bool {
        let existing = node.used_host_ports();
        for c in &pod.spec.containers {
            for port in &c.ports {
                if port.host_port == 0 {
                    continue;
                }
                let candidate = HostPortUse {
                    host_ip: port.host_ip.clone(),
                    host_port: port.host_port,
                    protocol: port.protocol,
                };
                if existing.iter().any(|e| conflicts(e, &candidate)) {
                    return false;
                }
            }
        }
        true
    }
}

/// Upstream: `pkg/scheduler/framework/types.go::HostPortInfo.CheckConflict`.
fn conflicts(a: &HostPortUse, b: &HostPortUse) -> bool {
    if a.host_port != b.host_port || a.protocol != b.protocol {
        return false;
    }
    // Empty / "0.0.0.0" listen-on-all collides with any specific IP.
    let a_any = a.host_ip.is_empty() || a.host_ip == "0.0.0.0";
    let b_any = b.host_ip.is_empty() || b.host_ip == "0.0.0.0";
    a_any || b_any || a.host_ip == b.host_ip
}

impl FilterPlugin for NodePorts {
    fn name(&self) -> &'static str {
        "NodePorts"
    }

    fn filter(&self, _state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> Status {
        if Self::fits(pod, node) {
            Status::success()
        } else {
            Status::unschedulable(self.name(), "node(s) didn't have free ports for the pod")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Container, ContainerPort, Node, Pod, Protocol};

    fn empty_node() -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        NodeInfo::new(n)
    }

    fn pod_with_port(hp: i32, ip: &str) -> Pod {
        let mut p = Pod::default();
        let mut c = Container::default();
        c.ports.push(ContainerPort {
            host_port: hp,
            container_port: hp,
            protocol: Protocol::Tcp,
            host_ip: ip.into(),
        });
        p.spec.containers.push(c);
        p
    }

    #[test]
    fn pod_with_no_host_ports_fits() {
        let info = empty_node();
        let p = Pod::default();
        let mut s = CycleState::new();
        assert!(NodePorts.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn same_port_on_any_ip_conflicts() {
        let mut info = empty_node();
        info.add_pod(pod_with_port(8080, ""));
        let new = pod_with_port(8080, "10.0.0.1");
        let mut s = CycleState::new();
        let st = NodePorts.filter(&mut s, &new, &info);
        assert!(!st.is_success());
    }

    #[test]
    fn same_port_different_protocol_does_not_conflict() {
        let mut info = empty_node();
        let mut tcp = pod_with_port(8080, "10.0.0.1");
        info.add_pod(tcp.clone());
        // UDP on the same IP/port is fine.
        let mut p = Pod::default();
        let mut c = Container::default();
        c.ports.push(ContainerPort {
            host_port: 8080,
            container_port: 8080,
            protocol: Protocol::Udp,
            host_ip: "10.0.0.1".into(),
        });
        p.spec.containers.push(c);
        let mut s = CycleState::new();
        assert!(NodePorts.filter(&mut s, &p, &info).is_success());

        // Silence the unused warning.
        tcp.spec.containers.clear();
    }
}
