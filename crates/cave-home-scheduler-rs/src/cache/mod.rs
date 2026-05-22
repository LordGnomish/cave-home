// SPDX-License-Identifier: Apache-2.0
//! Scheduler cache — aggregated node/pod state visible to plugins.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/backend/cache/interface.go
//!         pkg/scheduler/backend/cache/cache.go

pub mod assumed_pods;
pub mod node_info;

pub use assumed_pods::AssumedPodTracker;
pub use node_info::NodeInfo;

use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::types::{Node, Pod};

/// Errors emitted by the cache.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("node {0} not found in cache")]
    NodeNotFound(String),
    #[error("pod {0} already assumed on another node")]
    PodAlreadyAssumed(String),
}

pub type Result<T> = std::result::Result<T, CacheError>;

/// Upstream: `pkg/scheduler/backend/cache/interface.go::Cache`.
#[derive(Clone, Default)]
pub struct SchedulerCache {
    inner: Arc<RwLock<SchedulerCacheInner>>,
}

#[derive(Default)]
struct SchedulerCacheInner {
    nodes: BTreeMap<String, NodeInfo>,
    assumed: AssumedPodTracker,
}

impl SchedulerCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upstream: `Cache.AddNode`.
    pub fn add_node(&self, node: Node) {
        let mut g = self.inner.write();
        let info = g.nodes.entry(node.metadata.name.clone()).or_insert_with(|| {
            NodeInfo::new(node.clone())
        });
        info.set_node(node);
    }

    /// Upstream: `Cache.RemoveNode`.
    pub fn remove_node(&self, name: &str) {
        self.inner.write().nodes.remove(name);
    }

    /// Upstream: `Cache.UpdateNode`. Idempotent given `add_node` semantics.
    pub fn update_node(&self, node: Node) {
        self.add_node(node);
    }

    /// Upstream: `Cache.AddPod`. Records that a real bound pod resides on
    /// `pod.spec.node_name` and updates aggregated request totals.
    pub fn add_pod(&self, pod: Pod) -> Result<()> {
        if pod.spec.node_name.is_empty() {
            // Not actually placed yet — nothing to fold into a NodeInfo.
            return Ok(());
        }
        let mut g = self.inner.write();
        let info = g
            .nodes
            .get_mut(&pod.spec.node_name)
            .ok_or_else(|| CacheError::NodeNotFound(pod.spec.node_name.clone()))?;
        info.add_pod(pod);
        Ok(())
    }

    /// Upstream: `Cache.RemovePod`.
    pub fn remove_pod(&self, pod: &Pod) -> Result<()> {
        if pod.spec.node_name.is_empty() {
            return Ok(());
        }
        let mut g = self.inner.write();
        let info = g
            .nodes
            .get_mut(&pod.spec.node_name)
            .ok_or_else(|| CacheError::NodeNotFound(pod.spec.node_name.clone()))?;
        info.remove_pod(pod);
        Ok(())
    }

    /// Upstream: `Cache.AssumePod`. Records a tentative placement before
    /// the bind RPC; cleared by `forget_pod` on bind failure or by
    /// `add_pod` on bind success.
    pub fn assume_pod(&self, pod: Pod, node_name: &str) -> Result<()> {
        let key = pod.metadata.uid.clone();
        let mut g = self.inner.write();
        if g.assumed.contains(&key) {
            return Err(CacheError::PodAlreadyAssumed(key));
        }
        let info = g
            .nodes
            .get_mut(node_name)
            .ok_or_else(|| CacheError::NodeNotFound(node_name.into()))?;
        let mut assumed_pod = pod.clone();
        assumed_pod.spec.node_name = node_name.into();
        info.add_pod(assumed_pod.clone());
        g.assumed.insert(key, assumed_pod);
        Ok(())
    }

    /// Upstream: `Cache.ForgetPod`.
    pub fn forget_pod(&self, uid: &str) {
        let mut g = self.inner.write();
        if let Some(p) = g.assumed.remove(uid) {
            if let Some(info) = g.nodes.get_mut(&p.spec.node_name) {
                info.remove_pod(&p);
            }
        }
    }

    /// Snapshot all node infos, ordered by node name.
    /// Upstream: `Cache.UpdateSnapshot` (we always synthesise on demand).
    #[must_use]
    pub fn snapshot(&self) -> Vec<NodeInfo> {
        let g = self.inner.read();
        g.nodes.values().cloned().collect()
    }

    /// Fetch a single node info clone by name.
    #[must_use]
    pub fn node_info(&self, name: &str) -> Option<NodeInfo> {
        self.inner.read().nodes.get(name).cloned()
    }

    /// Total node count.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.inner.read().nodes.len()
    }

    /// True if the pod is currently assumed.
    #[must_use]
    pub fn is_assumed(&self, uid: &str) -> bool {
        self.inner.read().assumed.contains(uid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Node, ObjectMeta, Pod, Quantity, ResourceName};

    fn node(name: &str, cpu_m: i64, mem_b: i64) -> Node {
        let mut n = Node::default();
        n.metadata.name = name.into();
        n.status
            .allocatable
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
        n.status
            .allocatable
            .insert(ResourceName::Memory, Quantity::bytes(mem_b));
        n
    }

    fn pod(name: &str, cpu_m: i64, mem_b: i64) -> Pod {
        let mut p = Pod::default();
        p.metadata = ObjectMeta {
            name: name.into(),
            namespace: "default".into(),
            uid: name.into(),
            ..Default::default()
        };
        let mut c = crate::types::Container::default();
        c.resources
            .requests
            .insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
        c.resources
            .requests
            .insert(ResourceName::Memory, Quantity::bytes(mem_b));
        p.spec.containers.push(c);
        p
    }

    #[test]
    fn cache_starts_empty() {
        let c = SchedulerCache::new();
        assert_eq!(c.node_count(), 0);
        assert!(c.snapshot().is_empty());
    }

    #[test]
    fn add_node_and_query_node_info() {
        let c = SchedulerCache::new();
        c.add_node(node("n1", 1000, 1024));
        assert_eq!(c.node_count(), 1);
        let info = c.node_info("n1").unwrap();
        assert_eq!(info.node().metadata.name, "n1");
    }

    #[test]
    fn add_pod_updates_requested_resources() {
        let c = SchedulerCache::new();
        c.add_node(node("n1", 2000, 4096));
        let mut p = pod("alpha", 500, 1024);
        p.spec.node_name = "n1".into();
        c.add_pod(p).unwrap();
        let info = c.node_info("n1").unwrap();
        assert_eq!(info.requested(ResourceName::Cpu), 500);
        assert_eq!(info.requested(ResourceName::Memory), 1024);
    }

    #[test]
    fn remove_pod_reduces_requested() {
        let c = SchedulerCache::new();
        c.add_node(node("n1", 2000, 4096));
        let mut p = pod("alpha", 500, 1024);
        p.spec.node_name = "n1".into();
        c.add_pod(p.clone()).unwrap();
        c.remove_pod(&p).unwrap();
        let info = c.node_info("n1").unwrap();
        assert_eq!(info.requested(ResourceName::Cpu), 0);
    }

    #[test]
    fn assume_pod_marks_as_assumed_and_increments_requested() {
        let c = SchedulerCache::new();
        c.add_node(node("n1", 2000, 4096));
        let p = pod("alpha", 500, 1024);
        c.assume_pod(p.clone(), "n1").unwrap();
        assert!(c.is_assumed("alpha"));
        assert_eq!(c.node_info("n1").unwrap().requested(ResourceName::Cpu), 500);
    }

    #[test]
    fn assume_pod_twice_errors() {
        let c = SchedulerCache::new();
        c.add_node(node("n1", 2000, 4096));
        let p = pod("alpha", 500, 1024);
        c.assume_pod(p.clone(), "n1").unwrap();
        let err = c.assume_pod(p, "n1").unwrap_err();
        assert!(matches!(err, CacheError::PodAlreadyAssumed(_)));
    }

    #[test]
    fn forget_pod_releases_resources() {
        let c = SchedulerCache::new();
        c.add_node(node("n1", 2000, 4096));
        let p = pod("alpha", 500, 1024);
        c.assume_pod(p, "n1").unwrap();
        c.forget_pod("alpha");
        assert!(!c.is_assumed("alpha"));
        assert_eq!(c.node_info("n1").unwrap().requested(ResourceName::Cpu), 0);
    }

    #[test]
    fn remove_node_drops_it_from_snapshot() {
        let c = SchedulerCache::new();
        c.add_node(node("n1", 1000, 1024));
        c.add_node(node("n2", 2000, 2048));
        c.remove_node("n1");
        assert_eq!(c.snapshot().len(), 1);
        assert_eq!(c.snapshot()[0].node().metadata.name, "n2");
    }
}
