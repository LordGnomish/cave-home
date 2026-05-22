// SPDX-License-Identifier: Apache-2.0
//! Top-level composition.
//!
//! Hand-port of `pkg/kubelet/kubelet.go::syncPod` + the surrounding
//! orchestrator (v1.36.1). Phase 1 wires together:
//!
//! - `cri::CriClient`        — runtime backend (mock by default).
//! - `podworker::PodWorker`  — per-pod state machine; one per pod UID.
//! - `volume::Reconciler`    — DSW <-> ASW (emptyDir + hostPath).
//! - `status::PodStatusManager` — dedup queue flushed via `StatusSink`.
//!
//! `sync_pod` is the line-by-line analogue of `Kubelet.syncPod`:
//!   1. Register the pod's volumes in the DesiredStateOfWorld.
//!   2. Reconcile volumes (mount missing ones).
//!   3. Drive the PodWorker to reconcile sandbox + containers.
//!   4. Compose a PodStatus from the CRI snapshot and push it into the
//!      status manager.
//!   5. On `Terminated`, evict the pod from DSW and forget worker + status.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use thiserror::Error;

use crate::api::{
    ContainerState as ApiContainerState, ContainerStateRunning, ContainerStateTerminated,
    ContainerStatus, Pod, PodPhase, PodStatus, PodUid,
};
use crate::cri::types::{ContainerFilter, ContainerState as CriState};
use crate::cri::{CriClient, CriError};
use crate::podworker::{PodWorker, PodWorkerError, SyncOutcome, WorkType};
use crate::status::{PodStatusManager, StatusManagerError, StatusSink};
use crate::volume::emptydir::EmptyDirPlugin;
use crate::volume::hostpath::HostPathPlugin;
use crate::volume::plugin::{VolumeError, VolumePlugin};
use crate::volume::{ActualStateOfWorld, DesiredStateOfWorld, Reconciler};

#[derive(Debug, Error)]
pub enum KubeletError {
    #[error("CRI: {0}")]
    Cri(#[from] CriError),
    #[error("podworker: {0}")]
    PodWorker(#[from] PodWorkerError),
    #[error("status: {0}")]
    Status(#[from] StatusManagerError),
    #[error("volume: {0}")]
    Volume(#[from] VolumeError),
}

/// Phase-1 kubelet — composes CRI + PodWorkers + VolumeManager + Status.
pub struct Kubelet {
    cri: Arc<dyn CriClient>,
    workers: Mutex<HashMap<PodUid, Arc<PodWorker>>>,
    status_mgr: Arc<PodStatusManager>,
    desired_volumes: Arc<DesiredStateOfWorld>,
    actual_volumes: Arc<ActualStateOfWorld>,
    reconciler: Reconciler,
}

impl Kubelet {
    /// Construct with the production volume root.
    pub fn new(cri: Arc<dyn CriClient>, sink: Arc<dyn StatusSink>) -> Self {
        Self::with_volume_root(
            cri,
            sink,
            Path::new(crate::volume::emptydir::DEFAULT_PODS_ROOT),
        )
    }

    /// Construct a kubelet with an explicit volume-root (used by tests so
    /// emptyDir volumes land under a tempdir).
    pub fn with_volume_root(
        cri: Arc<dyn CriClient>,
        sink: Arc<dyn StatusSink>,
        volume_root: &Path,
    ) -> Self {
        let status_mgr = Arc::new(PodStatusManager::new(sink));
        let desired = Arc::new(DesiredStateOfWorld::new());
        let actual = Arc::new(ActualStateOfWorld::new());
        let plugins: Vec<Arc<dyn VolumePlugin>> = vec![
            Arc::new(EmptyDirPlugin::new(volume_root)),
            Arc::new(HostPathPlugin::new()),
        ];
        let reconciler = Reconciler::new(plugins, desired.clone(), actual.clone());
        Self {
            cri,
            workers: Mutex::new(HashMap::new()),
            status_mgr,
            desired_volumes: desired,
            actual_volumes: actual,
            reconciler,
        }
    }

    fn worker_for(&self, uid: &PodUid) -> Arc<PodWorker> {
        self.workers
            .lock()
            .entry(uid.clone())
            .or_insert_with(|| Arc::new(PodWorker::new(self.cri.clone())))
            .clone()
    }

    /// Drive one sync iteration.
    pub async fn sync_pod(&self, pod: &Pod, work: WorkType) -> Result<SyncOutcome, KubeletError> {
        let uid = pod.uid().clone();

        match work {
            WorkType::Sync => {
                // 1. Register desired volumes & reconcile mounts.
                self.desired_volumes.add_pod(uid.clone(), pod.spec.volumes.clone());
                self.reconciler.reconcile_once().await?;

                // 2. Drive worker.
                let worker = self.worker_for(&uid);
                let outcome = worker.sync(pod, WorkType::Sync).await?;

                // 3. Compose status from CRI snapshot & enqueue.
                let st = self.compose_status(pod, &outcome).await?;
                self.status_mgr.set_pod_status(&uid, st).await?;

                Ok(outcome)
            }
            WorkType::Terminating => {
                let worker = self.worker_for(&uid);
                let outcome = worker.sync(pod, WorkType::Terminating).await?;
                let st = self.compose_status(pod, &outcome).await?;
                self.status_mgr.set_pod_status(&uid, st).await?;
                Ok(outcome)
            }
            WorkType::Terminated => {
                let worker = self.worker_for(&uid);
                let outcome = worker.sync(pod, WorkType::Terminated).await?;
                self.desired_volumes.remove_pod(&uid);
                self.reconciler.reconcile_once().await?;
                self.status_mgr.forget_pod(&uid);
                self.workers.lock().remove(&uid);
                Ok(outcome)
            }
        }
    }

    /// Forget all kubelet-side state for a pod (used by external GC).
    pub fn forget_pod(&self, uid: &PodUid) {
        self.workers.lock().remove(uid);
        self.status_mgr.forget_pod(uid);
        self.desired_volumes.remove_pod(uid);
    }

    /// Flush pending status updates through the status sink.
    pub async fn flush_status(&self) -> Result<usize, KubeletError> {
        Ok(self.status_mgr.sync_batch().await?)
    }

    /// Snapshot the host path a volume was mounted at (test introspection).
    #[must_use]
    pub fn volume_host_path(&self, uid: &PodUid, name: &str) -> Option<PathBuf> {
        self.actual_volumes.get_host_path(uid, name)
    }

    /// Compose a PodStatus from the latest CRI snapshot, mirroring
    /// `pkg/kubelet/kubelet_pods.go::generateAPIPodStatus`.
    async fn compose_status(
        &self,
        pod: &Pod,
        outcome: &SyncOutcome,
    ) -> Result<PodStatus, KubeletError> {
        let mut container_statuses = Vec::with_capacity(pod.spec.containers.len());
        let mut all_running = !pod.spec.containers.is_empty();
        let mut any_failed = false;
        let mut all_succeeded = !pod.spec.containers.is_empty();

        if let Some(sandbox_id) = &outcome.sandbox_id {
            let cri_containers = self
                .cri
                .list_containers(Some(ContainerFilter {
                    pod_sandbox_id: Some(sandbox_id.clone()),
                    ..Default::default()
                }))
                .await?;
            for desired in &pod.spec.containers {
                if let Some(c) = cri_containers
                    .iter()
                    .find(|c| c.metadata.name == desired.name)
                {
                    let st = self.cri.container_status(&c.id).await?;
                    let api_state = match c.state {
                        CriState::Running => {
                            all_succeeded = false;
                            ApiContainerState::Running(ContainerStateRunning {
                                started_at_ms: st.started_at.max(0) as u64,
                            })
                        }
                        CriState::Exited => {
                            all_running = false;
                            if st.exit_code != 0 {
                                any_failed = true;
                                all_succeeded = false;
                            }
                            ApiContainerState::Terminated(ContainerStateTerminated {
                                exit_code: st.exit_code,
                                reason: st.reason.clone(),
                                message: st.message.clone(),
                                started_at_ms: st.started_at.max(0) as u64,
                                finished_at_ms: st.finished_at.max(0) as u64,
                            })
                        }
                        _ => {
                            all_running = false;
                            all_succeeded = false;
                            ApiContainerState::default()
                        }
                    };
                    container_statuses.push(ContainerStatus {
                        name: desired.name.clone(),
                        state: api_state,
                        image: desired.image.clone(),
                        container_id: Some(crate::api::ContainerId::new(&c.id)),
                        ready: c.state == CriState::Running,
                        restart_count: 0,
                    });
                } else {
                    all_running = false;
                    all_succeeded = false;
                    container_statuses.push(ContainerStatus {
                        name: desired.name.clone(),
                        image: desired.image.clone(),
                        ..Default::default()
                    });
                }
            }
        } else {
            all_running = false;
            all_succeeded = false;
        }

        let phase = if any_failed {
            PodPhase::Failed
        } else if all_succeeded {
            PodPhase::Succeeded
        } else if all_running {
            PodPhase::Running
        } else {
            PodPhase::Pending
        };

        Ok(PodStatus {
            phase,
            message: String::new(),
            reason: String::new(),
            container_statuses,
        })
    }
}
