// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         cmd/kube-controller-manager/app/controllermanager.go
//
//! Top-level controller-manager composition.
//!
//! Holds one handle per Phase 2 controller and wires them to the shared
//! [`crate::api_client::ControllerApiClient`] and per-controller
//! [`crate::workqueue::RateLimitingQueue`]s. The `run` method is a single
//! reconcile sweep over every queued key; production code drives it from a
//! tokio task loop.

use std::sync::Arc;

use crate::api_client::{ApiResult, ControllerApiClient};
use crate::controllers::cronjob::{Clock, CronJobController, SystemClock};
use crate::controllers::daemonset::DaemonSetController;
use crate::controllers::deployment::DeploymentController;
use crate::controllers::garbage_collector::GarbageCollector;
use crate::controllers::job::JobController;
use crate::controllers::namespace::NamespaceController;
use crate::controllers::node::NodeController;
use crate::controllers::replicaset::ReplicaSetController;
use crate::controllers::serviceaccount::{ServiceAccountController, TokenController};
use crate::controllers::statefulset::StatefulSetController;
use crate::workqueue::RateLimitingQueue;

/// Top-level controller-manager.
pub struct ControllerManager<C: ControllerApiClient + Send + Sync + 'static> {
    pub client: Arc<C>,
    pub deployments: DeploymentController<C>,
    pub replica_sets: ReplicaSetController<C>,
    pub daemon_sets: DaemonSetController<C>,
    pub stateful_sets: StatefulSetController<C>,
    pub jobs: JobController<C>,
    pub cron_jobs: CronJobController<C>,
    pub service_accounts: ServiceAccountController<C>,
    pub tokens: TokenController<C>,
    pub namespaces: NamespaceController<C>,
    pub nodes: NodeController<C>,
    pub garbage_collector: GarbageCollector<C>,

    pub deployment_queue: RateLimitingQueue<String>,
    pub replica_set_queue: RateLimitingQueue<String>,
    pub daemon_set_queue: RateLimitingQueue<String>,
    pub stateful_set_queue: RateLimitingQueue<String>,
    pub job_queue: RateLimitingQueue<String>,
    pub cron_job_queue: RateLimitingQueue<String>,
    pub namespace_queue: RateLimitingQueue<String>,
}

impl<C: ControllerApiClient + Send + Sync + 'static> ControllerManager<C> {
    /// Construct with the system clock (production builds).
    pub fn new(client: Arc<C>) -> Self {
        Self::with_clock(client, Arc::new(SystemClock))
    }

    /// Construct with a custom clock — tests pass a `FixedClock` here.
    pub fn with_clock(client: Arc<C>, clock: Arc<dyn Clock>) -> Self {
        Self {
            deployments: DeploymentController::new(Arc::clone(&client)),
            replica_sets: ReplicaSetController::new(Arc::clone(&client)),
            daemon_sets: DaemonSetController::new(Arc::clone(&client)),
            stateful_sets: StatefulSetController::new(Arc::clone(&client)),
            jobs: JobController::new(Arc::clone(&client)),
            cron_jobs: CronJobController::new(Arc::clone(&client), clock),
            service_accounts: ServiceAccountController::new(Arc::clone(&client)),
            tokens: TokenController::new(Arc::clone(&client)),
            namespaces: NamespaceController::new(Arc::clone(&client)),
            nodes: NodeController::new(Arc::clone(&client)),
            garbage_collector: GarbageCollector::new(Arc::clone(&client)),

            deployment_queue: RateLimitingQueue::new(),
            replica_set_queue: RateLimitingQueue::new(),
            daemon_set_queue: RateLimitingQueue::new(),
            stateful_set_queue: RateLimitingQueue::new(),
            job_queue: RateLimitingQueue::new(),
            cron_job_queue: RateLimitingQueue::new(),
            namespace_queue: RateLimitingQueue::new(),

            client,
        }
    }

    /// Process every currently-ready item from every queue once.
    ///
    /// Production code calls this in a `loop` — the queues block until a key
    /// is ready, so the loop is naturally rate-paced.
    pub async fn drain_once(&self) -> ApiResult<DrainStats> {
        let mut stats = DrainStats::default();
        while let Some(key) = self.deployment_queue.try_get() {
            self.deployments.reconcile(&key).await?;
            self.deployment_queue.done(&key);
            self.deployment_queue.forget(&key);
            stats.deployments += 1;
        }
        while let Some(key) = self.replica_set_queue.try_get() {
            self.replica_sets.reconcile(&key).await?;
            self.replica_set_queue.done(&key);
            self.replica_set_queue.forget(&key);
            stats.replica_sets += 1;
        }
        while let Some(key) = self.daemon_set_queue.try_get() {
            self.daemon_sets.reconcile(&key).await?;
            self.daemon_set_queue.done(&key);
            self.daemon_set_queue.forget(&key);
            stats.daemon_sets += 1;
        }
        while let Some(key) = self.stateful_set_queue.try_get() {
            self.stateful_sets.reconcile(&key).await?;
            self.stateful_set_queue.done(&key);
            self.stateful_set_queue.forget(&key);
            stats.stateful_sets += 1;
        }
        while let Some(key) = self.job_queue.try_get() {
            self.jobs.reconcile(&key).await?;
            self.job_queue.done(&key);
            self.job_queue.forget(&key);
            stats.jobs += 1;
        }
        while let Some(key) = self.cron_job_queue.try_get() {
            self.cron_jobs.reconcile(&key).await?;
            self.cron_job_queue.done(&key);
            self.cron_job_queue.forget(&key);
            stats.cron_jobs += 1;
        }
        while let Some(key) = self.namespace_queue.try_get() {
            self.namespaces.reconcile(&key).await?;
            self.namespace_queue.done(&key);
            self.namespace_queue.forget(&key);
            stats.namespaces += 1;
        }
        Ok(stats)
    }
}

/// Per-call statistics emitted by [`ControllerManager::drain_once`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DrainStats {
    pub deployments: u32,
    pub replica_sets: u32,
    pub daemon_sets: u32,
    pub stateful_sets: u32,
    pub jobs: u32,
    pub cron_jobs: u32,
    pub namespaces: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::InMemoryApiClient;
    use crate::types::{LabelSelector, ObjectMeta, PodTemplateSpec, ReplicaSet, ReplicaSetSpec};

    #[tokio::test]
    async fn drain_runs_replica_set_reconcile_for_queued_key() {
        let c = Arc::new(InMemoryApiClient::new());
        let mut sel = LabelSelector::default();
        sel.match_labels.insert("app".into(), "web".into());
        let mut tpl = PodTemplateSpec::default();
        tpl.metadata.labels.insert("app".into(), "web".into());
        c.seed(
            Some("default"),
            ReplicaSet {
                metadata: ObjectMeta {
                    name: "web".into(),
                    namespace: "default".into(),
                    ..Default::default()
                },
                spec: ReplicaSetSpec {
                    replicas: 2,
                    selector: sel,
                    template: tpl,
                },
                ..Default::default()
            },
        );
        let mgr = ControllerManager::new(c.clone());
        mgr.replica_set_queue.add("default/web".into());
        let stats = mgr.drain_once().await.unwrap();
        assert_eq!(stats.replica_sets, 1);
        assert_eq!(c.count("Pod"), 2);
    }

    #[tokio::test]
    async fn drain_returns_zero_stats_on_empty_queues() {
        let c = Arc::new(InMemoryApiClient::new());
        let mgr = ControllerManager::new(c);
        let stats = mgr.drain_once().await.unwrap();
        assert_eq!(stats, DrainStats::default());
    }
}
