// SPDX-License-Identifier: Apache-2.0
//! The controller manager run loop: wires the workload controllers to the
//! in-memory apiserver through a rate-limited [`WorkQueue`] per controller and a
//! resync-driven step loop.
//!
//! This is the analogue of `kube-controller-manager`'s shared run loop. In a
//! real deployment each controller registers informer event handlers that
//! enqueue affected keys; here that event flow is modelled by **resync** —
//! every step re-lists each resource and enqueues its keys (client-go informers
//! resync on a period regardless, so a resync-only loop is a faithful, if
//! eager, model of the same convergence). Each key is then drained through the
//! controller's [`WorkQueue`] and reconciled, and [`apply_outcome`] applies the
//! result (forget / requeue / rate-limited backoff) exactly as the real loop.
//!
//! Kubelet is simulated by [`Manager::admit_pods`]: pending pods that a
//! controller created become `Running`/ready, so availability propagates and
//! rollouts make progress — the cluster reaches a fixed point.

use crate::apis::{Cluster, PodPhase};
use crate::controllers::deployment::DeploymentController;
use crate::controllers::replicaset::ReplicaSetController;
use crate::reconcile::{apply_outcome, Outcome};
use crate::types::Object;
use crate::workqueue::WorkQueue;

/// A minimal controller manager: the in-memory [`Cluster`], the `Deployment`
/// and `ReplicaSet` controllers, and a [`WorkQueue`] for each.
#[derive(Debug)]
pub struct Manager {
    /// The shared in-memory apiserver every controller reads and writes.
    pub cluster: Cluster,
    deploy_ctrl: DeploymentController,
    rs_ctrl: ReplicaSetController,
    deploy_q: WorkQueue,
    rs_q: WorkQueue,
}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

impl Manager {
    /// A manager over a fresh, empty cluster.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cluster: Cluster::new(),
            deploy_ctrl: DeploymentController::new(),
            rs_ctrl: ReplicaSetController::new(),
            deploy_q: WorkQueue::with_defaults(),
            rs_q: WorkQueue::with_defaults(),
        }
    }

    /// Run resync → reconcile → admit → reconcile rounds until the cluster
    /// reaches a fixed point or `max_rounds` is hit. Returns the number of
    /// rounds taken (== `max_rounds` if it did not converge).
    pub fn run_until_stable(&mut self, now: u64, max_rounds: usize) -> usize {
        for round in 0..max_rounds {
            let before = self.fingerprint();
            self.step(now);
            if self.fingerprint() == before {
                return round + 1;
            }
        }
        max_rounds
    }

    /// One convergence step: reconcile `Deployment`s (which scale
    /// `ReplicaSet`s), reconcile `ReplicaSet`s (which create/delete `Pod`s),
    /// admit pending pods, then reconcile both again so observed status
    /// propagates upward.
    pub fn step(&mut self, now: u64) {
        self.reconcile_deployments(now);
        self.reconcile_replicasets(now);
        self.admit_pods();
        self.reconcile_replicasets(now);
        self.reconcile_deployments(now);
    }

    /// Resync + drain the Deployment queue.
    fn reconcile_deployments(&mut self, now: u64) {
        for d in self.cluster.deployments.list() {
            self.deploy_q.add(&d.key());
        }
        while let Some(key) = self.deploy_q.get(now) {
            let outcome = self.deploy_ctrl.reconcile(&key, &mut self.cluster, now);
            debug_assert!(matches!(outcome, Outcome::Done));
            apply_outcome(&mut self.deploy_q, &key, &outcome, now);
        }
    }

    /// Resync + drain the `ReplicaSet` queue.
    fn reconcile_replicasets(&mut self, now: u64) {
        for r in self.cluster.replicasets.list() {
            self.rs_q.add(&r.key());
        }
        while let Some(key) = self.rs_q.get(now) {
            let outcome = self.rs_ctrl.reconcile(&key, &mut self.cluster, now);
            apply_outcome(&mut self.rs_q, &key, &outcome, now);
        }
    }

    /// Simulate kubelet admitting pods: every active `Pending` pod transitions
    /// to `Running` + ready. Returns whether anything changed.
    pub fn admit_pods(&mut self) -> bool {
        let mut changed = false;
        for mut pod in self.cluster.pods.list() {
            if pod.is_active() && pod.status.phase == PodPhase::Pending {
                pod.status.phase = PodPhase::Running;
                pod.status.ready = true;
                self.cluster.pods.update(pod);
                changed = true;
            }
        }
        changed
    }

    /// A stable, order-independent rendering of the cluster's load-bearing
    /// state, used to detect convergence between steps.
    fn fingerprint(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for d in self.cluster.deployments.list() {
            lines.push(format!(
                "d {} r={} st={}/{}/{}",
                d.key(),
                d.spec.replicas,
                d.status.replicas,
                d.status.ready_replicas,
                d.status.available_replicas
            ));
        }
        for r in self.cluster.replicasets.list() {
            lines.push(format!(
                "r {} r={} st={}/{}",
                r.key(),
                r.spec.replicas,
                r.status.replicas,
                r.status.available_replicas
            ));
        }
        for p in self.cluster.pods.list() {
            lines.push(format!("p {} {:?} ready={}", p.key(), p.status.phase, p.status.ready));
        }
        lines.sort();
        lines.join("\n")
    }
}
