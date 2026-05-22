// SPDX-License-Identifier: Apache-2.0
//! Per-node aggregated state visible to filter / score plugins.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/types.go::NodeInfo

use std::collections::BTreeMap;

use crate::types::{Node, Pod, Quantity, ResourceName};

/// Upstream: `pkg/scheduler/framework/types.go::NodeInfo`.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    node: Node,
    pods: Vec<Pod>,
    requested: BTreeMap<ResourceName, i64>,
    used_host_ports: Vec<HostPortUse>,
}

/// Upstream: `pkg/scheduler/framework/types.go::HostPortInfo`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HostPortUse {
    pub host_ip: String,
    pub host_port: i32,
    pub protocol: crate::types::Protocol,
}

impl NodeInfo {
    /// Upstream: `pkg/scheduler/framework/types.go::NewNodeInfo`.
    #[must_use]
    pub fn new(node: Node) -> Self {
        let mut me = Self {
            node,
            pods: Vec::new(),
            requested: BTreeMap::new(),
            used_host_ports: Vec::new(),
        };
        me.requested.insert(ResourceName::Cpu, 0);
        me.requested.insert(ResourceName::Memory, 0);
        me
    }

    /// Upstream: `NodeInfo.SetNode`.
    pub fn set_node(&mut self, node: Node) {
        self.node = node;
    }

    #[must_use]
    pub fn node(&self) -> &Node {
        &self.node
    }

    #[must_use]
    pub fn pods(&self) -> &[Pod] {
        &self.pods
    }

    #[must_use]
    pub fn used_host_ports(&self) -> &[HostPortUse] {
        &self.used_host_ports
    }

    #[must_use]
    pub fn requested(&self, r: ResourceName) -> i64 {
        self.requested.get(&r).copied().unwrap_or(0)
    }

    /// Upstream: `NodeInfo.AddPod`.
    pub fn add_pod(&mut self, pod: Pod) {
        for c in &pod.spec.containers {
            for (k, q) in &c.resources.requests {
                *self.requested.entry(*k).or_insert(0) += q.0;
            }
            for port in &c.ports {
                if port.host_port == 0 {
                    continue;
                }
                self.used_host_ports.push(HostPortUse {
                    host_ip: port.host_ip.clone(),
                    host_port: port.host_port,
                    protocol: port.protocol,
                });
            }
        }
        self.pods.push(pod);
    }

    /// Upstream: `NodeInfo.RemovePod`.
    pub fn remove_pod(&mut self, pod: &Pod) {
        if let Some(idx) = self.pods.iter().position(|p| p.metadata.uid == pod.metadata.uid) {
            let removed = self.pods.remove(idx);
            for c in &removed.spec.containers {
                for (k, q) in &c.resources.requests {
                    if let Some(slot) = self.requested.get_mut(k) {
                        *slot = (*slot).saturating_sub(q.0);
                    }
                }
                for port in &c.ports {
                    if port.host_port == 0 {
                        continue;
                    }
                    let target = HostPortUse {
                        host_ip: port.host_ip.clone(),
                        host_port: port.host_port,
                        protocol: port.protocol,
                    };
                    if let Some(pos) = self.used_host_ports.iter().position(|p| p == &target) {
                        self.used_host_ports.remove(pos);
                    }
                }
            }
        }
    }

    /// Upstream: `NodeInfo.Allocatable`.
    #[must_use]
    pub fn allocatable(&self, r: ResourceName) -> Quantity {
        self.node.allocatable(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Container, ContainerPort, ObjectMeta, Pod, PodSpec, Protocol, Quantity, ResourceName,
    };

    fn node() -> Node {
        let mut n = Node::default();
        n.metadata.name = "n".into();
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(4000));
        n.status
            .allocatable
            .insert(ResourceName::Memory, Quantity::bytes(8 * 1024));
        n
    }

    fn pod_with_ports(name: &str, hp: i32) -> Pod {
        let mut p = Pod::default();
        p.metadata = ObjectMeta {
            name: name.into(),
            uid: name.into(),
            ..Default::default()
        };
        p.spec = PodSpec::default();
        let mut c = Container::default();
        c.resources
            .requests
            .insert(ResourceName::Cpu, Quantity::milli_cpu(100));
        c.ports.push(ContainerPort {
            host_port: hp,
            container_port: hp,
            protocol: Protocol::Tcp,
            host_ip: String::new(),
        });
        p.spec.containers.push(c);
        p
    }

    #[test]
    fn add_and_remove_pod_round_trips_requested_cpu() {
        let mut info = NodeInfo::new(node());
        let p = pod_with_ports("a", 0);
        info.add_pod(p.clone());
        assert_eq!(info.requested(ResourceName::Cpu), 100);
        info.remove_pod(&p);
        assert_eq!(info.requested(ResourceName::Cpu), 0);
    }

    #[test]
    fn add_pod_with_host_port_records_use() {
        let mut info = NodeInfo::new(node());
        info.add_pod(pod_with_ports("a", 8080));
        assert_eq!(info.used_host_ports().len(), 1);
        assert_eq!(info.used_host_ports()[0].host_port, 8080);
    }

    #[test]
    fn remove_pod_clears_its_host_ports() {
        let mut info = NodeInfo::new(node());
        let p = pod_with_ports("a", 8080);
        info.add_pod(p.clone());
        info.remove_pod(&p);
        assert!(info.used_host_ports().is_empty());
    }
}
