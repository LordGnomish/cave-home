// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         pkg/controller/nodelifecycle/node_lifecycle_controller.go
//         pkg/controller/nodelifecycle/scheduler/taint_manager.go
//
//! NodeController (lifecycle).
//!
//! Watches `Node.status.conditions` heartbeat timestamps; if the last
//! `Ready` heartbeat is older than `node_monitor_grace_period`, the
//! controller flips Ready to Unknown and adds the
//! `node.kubernetes.io/not-ready:NoExecute` taint. The MemoryPressure /
//! DiskPressure conditions follow the same pattern.

use std::sync::Arc;
use std::time::Duration;

use crate::api_client::{ApiResult, ControllerApiClient};
use crate::types::{
    ConditionStatus, Node, NodeConditionType, Taint, TaintEffect,
};

/// Well-known taint keys mirrored from
/// `pkg/apis/core/well_known_taints.go`.
pub mod taints {
    pub const NOT_READY: &str = "node.kubernetes.io/not-ready";
    pub const UNREACHABLE: &str = "node.kubernetes.io/unreachable";
    pub const MEMORY_PRESSURE: &str = "node.kubernetes.io/memory-pressure";
    pub const DISK_PRESSURE: &str = "node.kubernetes.io/disk-pressure";
}

/// Configuration mirroring the upstream flags of the same name.
#[derive(Clone, Copy, Debug)]
pub struct NodeMonitorConfig {
    pub grace_period: Duration,
}

impl Default for NodeMonitorConfig {
    fn default() -> Self {
        // Upstream default is `nodeMonitorGracePeriod = 40s`.
        Self {
            grace_period: Duration::from_secs(40),
        }
    }
}

/// Reconcile the lifecycle of a single node.
///
/// Mirrors `Controller.monitorNodeHealth` for one node.
pub async fn sync_node<C: ControllerApiClient>(
    client: &C,
    cfg: NodeMonitorConfig,
    now_unix_ms: u64,
    node_name: &str,
) -> ApiResult<Node> {
    let mut node: Node = client.get(None, node_name).await?;
    let grace_ms = cfg.grace_period.as_millis() as u64;

    // 1) For each tracked condition (Ready/MemoryPressure/DiskPressure), if
    //    the last heartbeat is older than `grace_period`, flip status to
    //    Unknown.
    for kind in [
        NodeConditionType::Ready,
        NodeConditionType::MemoryPressure,
        NodeConditionType::DiskPressure,
    ] {
        if let Some(idx) = node.status.conditions.iter().position(|c| c.kind == kind) {
            let cond = &mut node.status.conditions[idx];
            if now_unix_ms.saturating_sub(cond.last_heartbeat_ms) > grace_ms
                && cond.status != ConditionStatus::Unknown
            {
                cond.status = ConditionStatus::Unknown;
                cond.reason = "NodeStatusUnknown".into();
                cond.message = "Kubelet stopped posting node status.".into();
                cond.last_transition_ms = now_unix_ms;
            }
        }
    }

    // 2) Sync taints to the Ready condition.
    let ready = node
        .status
        .condition(NodeConditionType::Ready)
        .map(|c| c.status)
        .unwrap_or(ConditionStatus::Unknown);
    let needs_not_ready_taint = match ready {
        ConditionStatus::True => false,
        ConditionStatus::False | ConditionStatus::Unknown => true,
    };
    if needs_not_ready_taint {
        ensure_taint(
            &mut node,
            Taint {
                key: taints::NOT_READY.into(),
                value: String::new(),
                effect: TaintEffect::NoExecute,
            },
        );
    } else {
        remove_taint(&mut node, taints::NOT_READY);
    }

    sync_pressure_taint(
        &mut node,
        NodeConditionType::MemoryPressure,
        taints::MEMORY_PRESSURE,
    );
    sync_pressure_taint(
        &mut node,
        NodeConditionType::DiskPressure,
        taints::DISK_PRESSURE,
    );

    client.update(None, node).await
}

fn sync_pressure_taint(node: &mut Node, kind: NodeConditionType, key: &str) {
    let pressed = node
        .status
        .condition(kind)
        .map(|c| c.status == ConditionStatus::True)
        .unwrap_or(false);
    if pressed {
        ensure_taint(
            node,
            Taint {
                key: key.into(),
                value: String::new(),
                effect: TaintEffect::NoSchedule,
            },
        );
    } else {
        remove_taint(node, key);
    }
}

fn ensure_taint(node: &mut Node, taint: Taint) {
    if !node
        .spec
        .taints
        .iter()
        .any(|t| t.key == taint.key && t.effect == taint.effect)
    {
        node.spec.taints.push(taint);
    }
}

fn remove_taint(node: &mut Node, key: &str) {
    node.spec.taints.retain(|t| t.key != key);
}

pub struct NodeController<C: ControllerApiClient> {
    client: Arc<C>,
    config: NodeMonitorConfig,
}

impl<C: ControllerApiClient> NodeController<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            config: NodeMonitorConfig::default(),
        }
    }

    pub fn with_config(mut self, cfg: NodeMonitorConfig) -> Self {
        self.config = cfg;
        self
    }

    pub async fn reconcile(&self, node_name: &str, now_unix_ms: u64) -> ApiResult<()> {
        sync_node(self.client.as_ref(), self.config, now_unix_ms, node_name).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::{NodeCondition, NodeStatus, ObjectMeta};

    fn make_node(name: &str, ready_status: ConditionStatus, last_hb: u64) -> Node {
        Node {
            metadata: ObjectMeta {
                name: name.into(),
                ..Default::default()
            },
            spec: Default::default(),
            status: NodeStatus {
                conditions: vec![NodeCondition {
                    kind: NodeConditionType::Ready,
                    status: ready_status,
                    reason: String::new(),
                    message: String::new(),
                    last_transition_ms: last_hb,
                    last_heartbeat_ms: last_hb,
                }],
            },
        }
    }

    #[tokio::test]
    async fn fresh_ready_node_carries_no_not_ready_taint() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_node("n1", ConditionStatus::True, 10_000));
        let n = sync_node(&c, NodeMonitorConfig::default(), 10_000, "n1")
            .await
            .unwrap();
        assert!(n.spec.taints.is_empty());
    }

    #[tokio::test]
    async fn stale_heartbeat_flips_ready_to_unknown() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_node("n1", ConditionStatus::True, 10_000));
        // 60s later (grace_period default = 40s).
        let n = sync_node(&c, NodeMonitorConfig::default(), 70_000, "n1")
            .await
            .unwrap();
        let ready = n.status.condition(NodeConditionType::Ready).unwrap();
        assert_eq!(ready.status, ConditionStatus::Unknown);
    }

    #[tokio::test]
    async fn unknown_ready_adds_not_ready_taint() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_node("n1", ConditionStatus::True, 10_000));
        let n = sync_node(&c, NodeMonitorConfig::default(), 70_000, "n1")
            .await
            .unwrap();
        assert!(n
            .spec
            .taints
            .iter()
            .any(|t| t.key == taints::NOT_READY && t.effect == TaintEffect::NoExecute));
    }

    #[tokio::test]
    async fn recovery_removes_not_ready_taint() {
        let c = InMemoryApiClient::new();
        c.seed(None, make_node("n1", ConditionStatus::True, 10_000));
        sync_node(&c, NodeMonitorConfig::default(), 70_000, "n1")
            .await
            .unwrap(); // tainted now
        // Heartbeat updates: refresh heartbeat to now.
        let mut n = c.get::<Node>(None, "n1").await.unwrap();
        let cond = n.status.conditions.iter_mut().next().unwrap();
        cond.status = ConditionStatus::True;
        cond.last_heartbeat_ms = 100_000;
        c.update(None, n).await.unwrap();
        let n = sync_node(&c, NodeMonitorConfig::default(), 100_000, "n1")
            .await
            .unwrap();
        assert!(n.spec.taints.iter().all(|t| t.key != taints::NOT_READY));
    }

    #[tokio::test]
    async fn memory_pressure_adds_taint() {
        let c = InMemoryApiClient::new();
        let mut n = make_node("n1", ConditionStatus::True, 10_000);
        n.status.conditions.push(NodeCondition {
            kind: NodeConditionType::MemoryPressure,
            status: ConditionStatus::True,
            reason: String::new(),
            message: String::new(),
            last_transition_ms: 10_000,
            last_heartbeat_ms: 10_000,
        });
        c.seed(None, n);
        let n = sync_node(&c, NodeMonitorConfig::default(), 10_000, "n1")
            .await
            .unwrap();
        assert!(n.spec.taints.iter().any(|t| t.key == taints::MEMORY_PRESSURE));
    }

    #[tokio::test]
    async fn disk_pressure_adds_taint() {
        let c = InMemoryApiClient::new();
        let mut n = make_node("n1", ConditionStatus::True, 10_000);
        n.status.conditions.push(NodeCondition {
            kind: NodeConditionType::DiskPressure,
            status: ConditionStatus::True,
            reason: String::new(),
            message: String::new(),
            last_transition_ms: 10_000,
            last_heartbeat_ms: 10_000,
        });
        c.seed(None, n);
        let n = sync_node(&c, NodeMonitorConfig::default(), 10_000, "n1")
            .await
            .unwrap();
        assert!(n.spec.taints.iter().any(|t| t.key == taints::DISK_PRESSURE));
    }
}
