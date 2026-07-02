// SPDX-License-Identifier: Apache-2.0
//! The pod lifecycle that turns a `kubectl apply`-ed Pod into a *running* one.
//!
//! Two real control loops, wired into [`crate::server`]'s supervisors, close the
//! gap between "an object exists in the apiserver store" and "its containers are
//! running":
//!
//! 1. **scheduling** ([`bind_pending_pods`]) — the single-node scheduler bind.
//!    Any Pod with an empty `spec.nodeName` is bound to the local node (this is a
//!    one-node home cluster, so every pod lands here). Behavioural reference:
//!    `pkg/scheduler` Bind; the full predicate/priority framework lives in
//!    `cave-home-scheduler-rs`, which a later pass can bridge to this store.
//!
//! 2. **kubelet sync** ([`PodRuntime::reconcile`]) — for every Pod bound to this
//!    node that is not yet `Running`, drive the real
//!    [`MockCriClient`](cave_home_kubelet_rs::cri::MockCriClient) through the CRI
//!    pod-sandbox + container lifecycle (`RunPodSandbox` → `PullImage` →
//!    `CreateContainer` → `StartContainer`), then write the observed `Running`
//!    status back to the apiserver. Behavioural reference:
//!    `pkg/kubelet` syncPod against the runtime. The mock CRI is the in-process
//!    runtime; swapping in the real containerd gRPC client (the `remote-cri`
//!    feature of `cave-home-kubelet-rs`) is the only change needed for real
//!    containers.
//!
//! The registry-mutating half ([`bind_pending_pods`], the status writeback) is
//! pure and synchronous so it is unit-testable; only the CRI calls are async.

use cave_home_apiserver_rs::gvk::GroupVersionResource;
use cave_home_apiserver_rs::json::{obj, Value};
use cave_home_apiserver_rs::registry::{ListOptions, Registry};
use cave_home_kubelet_rs::cri::types::{
    ContainerConfig, ContainerMetadata, ImageSpec, PodSandboxConfig, PodSandboxMetadata,
};
use cave_home_kubelet_rs::cri::{CriClient, MockCriClient};

use crate::server::SharedRegistry;

/// The `pods` GVR (core group, v1).
fn pods_gvr() -> GroupVersionResource {
    GroupVersionResource::new("", "v1", "pods")
}

/// A pod's `spec.nodeName`, or `""` if unbound.
fn node_name_of(pod: &Value) -> &str {
    pod.pointer("spec.nodeName").and_then(Value::as_str).unwrap_or("")
}

/// A pod's `status.phase`, or `""` if unset.
fn phase_of(pod: &Value) -> &str {
    pod.pointer("status.phase").and_then(Value::as_str).unwrap_or("")
}

/// The single-node scheduler bind: assign every unscheduled Pod to `node_name`.
///
/// Returns the number of pods newly bound. A pod is "unscheduled" when its
/// `spec.nodeName` is empty; binding patches it to `node_name` (and bumps the
/// pod's generation, exactly as a real Bind subresource write would).
pub fn bind_pending_pods(reg: &mut Registry, node_name: &str) -> usize {
    let pending: Vec<(String, String)> = reg
        .list(&pods_gvr(), &ListOptions::default())
        .map(|l| l.items)
        .unwrap_or_default()
        .iter()
        .filter(|p| node_name_of(p).is_empty())
        .filter_map(|p| Some((namespace_of(p), name_of(p)?)))
        .collect();

    let mut bound = 0;
    for (namespace, name) in pending {
        let patch = obj([("spec", obj([("nodeName", Value::from(node_name))]))]);
        if reg.patch_merge(&pods_gvr(), &namespace, &name, &patch).is_ok() {
            bound += 1;
        }
    }
    bound
}

/// A pod's `metadata.namespace`, defaulting to `default`.
fn namespace_of(pod: &Value) -> String {
    pod.pointer("metadata.namespace")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string()
}

/// A pod's `metadata.name` (None if absent — an unnameable object is skipped).
fn name_of(pod: &Value) -> Option<String> {
    pod.pointer("metadata.name").and_then(Value::as_str).map(str::to_string)
}

/// One container the kubelet must run: its name and image.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ContainerPlan {
    name: String,
    image: String,
}

/// One pod the kubelet must converge to Running.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PodPlan {
    namespace: String,
    name: String,
    uid: String,
    containers: Vec<ContainerPlan>,
}

/// Collect the pods bound to `node_name` that are not yet `Running` — the
/// kubelet's work-list for one sync pass.
fn runnable_pods(reg: &Registry, node_name: &str) -> Vec<PodPlan> {
    reg.list(&pods_gvr(), &ListOptions::default())
        .map(|l| l.items)
        .unwrap_or_default()
        .iter()
        .filter(|p| node_name_of(p) == node_name && phase_of(p) != "Running")
        .filter_map(|p| {
            Some(PodPlan {
                namespace: namespace_of(p),
                name: name_of(p)?,
                uid: p.pointer("metadata.uid").and_then(Value::as_str).unwrap_or("").to_string(),
                containers: read_containers(p),
            })
        })
        .collect()
}

/// Extract `spec.containers[*].{name,image}`.
fn read_containers(pod: &Value) -> Vec<ContainerPlan> {
    pod.pointer("spec.containers")
        .and_then(Value::as_array)
        .map(|cs| {
            cs.iter()
                .filter_map(|c| {
                    Some(ContainerPlan {
                        name: c.get("name").and_then(Value::as_str)?.to_string(),
                        image: c.get("image").and_then(Value::as_str).unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The node-local kubelet runtime: owns the CRI client and converges bound pods.
pub struct PodRuntime {
    cri: MockCriClient,
}

impl Default for PodRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl PodRuntime {
    /// A runtime backed by a fresh in-memory mock CRI.
    #[must_use]
    pub fn new() -> Self {
        Self { cri: MockCriClient::new() }
    }

    /// One kubelet sync pass: run every not-yet-`Running` pod bound to
    /// `node_name` through the CRI, then write its `Running` status back to the
    /// apiserver. Returns the number of pods newly brought up.
    ///
    /// The registry lock is never held across an `await`: pods are snapshotted
    /// under the lock, the CRI is driven without it, and the status writeback
    /// re-takes the lock per pod.
    pub async fn reconcile(&self, reg: &SharedRegistry, node_name: &str) -> usize {
        let plans = {
            let r = reg.lock().await;
            runnable_pods(&r, node_name)
        };
        let mut ran = 0;
        for plan in plans {
            if self.run_pod(&plan).await.is_ok() {
                let mut r = reg.lock().await;
                mark_running(&mut r, &plan);
                ran += 1;
            }
        }
        ran
    }

    /// Drive one pod through the CRI sandbox + container lifecycle.
    async fn run_pod(&self, plan: &PodPlan) -> cave_home_kubelet_rs::cri::CriResult<()> {
        let sandbox_cfg = PodSandboxConfig {
            metadata: PodSandboxMetadata {
                name: plan.name.clone(),
                uid: plan.uid.clone(),
                namespace: plan.namespace.clone(),
                attempt: 0,
            },
            ..PodSandboxConfig::default()
        };
        let sandbox_id = self.cri.run_pod_sandbox(sandbox_cfg.clone()).await?;
        for c in &plan.containers {
            self.cri.pull_image(ImageSpec { image: c.image.clone() }).await?;
            let cfg = ContainerConfig {
                metadata: ContainerMetadata { name: c.name.clone(), attempt: 0 },
                image: ImageSpec { image: c.image.clone() },
                ..ContainerConfig::default()
            };
            let cid = self.cri.create_container(&sandbox_id, cfg, sandbox_cfg.clone()).await?;
            self.cri.start_container(&cid).await?;
        }
        Ok(())
    }
}

/// Write the observed `Running` status (phase + conditions + per-container
/// statuses) onto the pod via a merge patch — the kubelet status update.
fn mark_running(reg: &mut Registry, plan: &PodPlan) {
    let container_statuses: Vec<Value> = plan
        .containers
        .iter()
        .map(|c| {
            obj([
                ("name", Value::from(c.name.as_str())),
                ("image", Value::from(c.image.as_str())),
                ("ready", Value::from(true)),
                ("started", Value::from(true)),
                ("restartCount", Value::from(0_i64)),
                // A real RFC3339 start time — kubectl parses this field (e.g. for
                // `kubectl logs`), so a placeholder string makes it error.
                ("state", obj([("running", obj([("startedAt", Value::from(crate::table::now_rfc3339()))]))])),
            ])
        })
        .collect();
    let condition = |t: &str| obj([("type", Value::from(t)), ("status", Value::from("True"))]);
    let patch = obj([(
        "status",
        obj([
            ("phase", Value::from("Running")),
            (
                "conditions",
                Value::Array(vec![
                    condition("PodScheduled"),
                    condition("Initialized"),
                    condition("ContainersReady"),
                    condition("Ready"),
                ]),
            ),
            ("containerStatuses", Value::Array(container_statuses)),
        ]),
    )]);
    let _ = reg.patch_merge(&pods_gvr(), &plan.namespace, &plan.name, &patch);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn pod(name: &str, node: &str) -> Value {
        let mut spec = obj([(
            "containers",
            Value::Array(vec![obj([
                ("name", Value::from("web")),
                ("image", Value::from("nginx:1.27")),
            ])]),
        )]);
        if !node.is_empty() {
            spec.insert("nodeName", Value::from(node));
        }
        obj([
            ("apiVersion", Value::from("v1")),
            ("kind", Value::from("Pod")),
            ("metadata", obj([("name", Value::from(name)), ("namespace", Value::from("default"))])),
            ("spec", spec),
        ])
    }

    fn seed(reg: &mut Registry, p: Value) {
        reg.create(&pods_gvr(), p).expect("create pod");
    }

    #[test]
    fn binds_an_unscheduled_pod_to_the_node() {
        let mut reg = Registry::new();
        seed(&mut reg, pod("nginx", ""));
        assert_eq!(bind_pending_pods(&mut reg, "hub-01"), 1);
        let got = reg.get(&pods_gvr(), "default", "nginx").expect("get");
        assert_eq!(node_name_of(&got), "hub-01");
        // idempotent: already bound → nothing to do.
        assert_eq!(bind_pending_pods(&mut reg, "hub-01"), 0);
    }

    #[test]
    fn does_not_rebind_an_already_scheduled_pod() {
        let mut reg = Registry::new();
        seed(&mut reg, pod("nginx", "other-node"));
        assert_eq!(bind_pending_pods(&mut reg, "hub-01"), 0);
        let got = reg.get(&pods_gvr(), "default", "nginx").expect("get");
        assert_eq!(node_name_of(&got), "other-node");
    }

    #[tokio::test]
    async fn kubelet_runs_a_bound_pod_to_running() {
        let mut reg = Registry::new();
        seed(&mut reg, pod("nginx", "hub-01"));
        let shared: SharedRegistry = Arc::new(Mutex::new(reg));

        let runtime = PodRuntime::new();
        assert_eq!(runtime.reconcile(&shared, "hub-01").await, 1);

        let got = shared.lock().await.get(&pods_gvr(), "default", "nginx").expect("get");
        assert_eq!(phase_of(&got), "Running");
        let statuses = got.pointer("status.containerStatuses").and_then(Value::as_array).expect("statuses");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].get("ready").and_then(Value::as_bool), Some(true));

        // A second pass is a no-op: the pod is already Running.
        assert_eq!(runtime.reconcile(&shared, "hub-01").await, 0);
    }

    #[tokio::test]
    async fn kubelet_ignores_pods_bound_to_other_nodes() {
        let mut reg = Registry::new();
        seed(&mut reg, pod("nginx", "other-node"));
        let shared: SharedRegistry = Arc::new(Mutex::new(reg));
        let runtime = PodRuntime::new();
        assert_eq!(runtime.reconcile(&shared, "hub-01").await, 0);
        let got = shared.lock().await.get(&pods_gvr(), "default", "nginx").expect("get");
        assert_eq!(phase_of(&got), "");
    }

    #[tokio::test]
    async fn schedule_then_run_is_end_to_end() {
        // Mirrors `kubectl apply`: an unscheduled pod becomes Running after one
        // scheduler bind followed by one kubelet sync.
        let mut reg = Registry::new();
        seed(&mut reg, pod("nginx", ""));
        assert_eq!(bind_pending_pods(&mut reg, "hub-01"), 1);
        let shared: SharedRegistry = Arc::new(Mutex::new(reg));
        let runtime = PodRuntime::new();
        assert_eq!(runtime.reconcile(&shared, "hub-01").await, 1);
        let got = shared.lock().await.get(&pods_gvr(), "default", "nginx").expect("get");
        assert_eq!(phase_of(&got), "Running");
    }
}
