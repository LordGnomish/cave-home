// SPDX-License-Identifier: Apache-2.0
//! `NodeName` — `Pod.Spec.NodeName` must equal `Node.Name`.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/plugins/nodename/node_name.go::Filter

use crate::cache::NodeInfo;
use crate::framework::{CycleState, FilterPlugin, Status};
use crate::types::Pod;

pub struct NodeName;

impl FilterPlugin for NodeName {
    fn name(&self) -> &'static str {
        "NodeName"
    }

    fn filter(&self, _state: &mut CycleState, pod: &Pod, node: &NodeInfo) -> Status {
        if pod.spec.node_name.is_empty() {
            return Status::success();
        }
        if pod.spec.node_name == node.node().metadata.name {
            Status::success()
        } else {
            Status::unresolvable(
                self.name(),
                format!(
                    "node(s) didn't match the requested node name {}",
                    pod.spec.node_name
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeInfo;
    use crate::types::{Node, Pod};

    fn node(name: &str) -> NodeInfo {
        let mut n = Node::default();
        n.metadata.name = name.into();
        NodeInfo::new(n)
    }

    #[test]
    fn empty_node_name_always_passes() {
        let info = node("any");
        let p = Pod::default();
        let mut s = CycleState::new();
        assert!(NodeName.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn matching_node_name_passes() {
        let info = node("foo");
        let mut p = Pod::default();
        p.spec.node_name = "foo".into();
        let mut s = CycleState::new();
        assert!(NodeName.filter(&mut s, &p, &info).is_success());
    }

    #[test]
    fn mismatched_node_name_fails() {
        let info = node("foo");
        let mut p = Pod::default();
        p.spec.node_name = "bar".into();
        let mut s = CycleState::new();
        let st = NodeName.filter(&mut s, &p, &info);
        assert!(!st.is_success());
    }
}
