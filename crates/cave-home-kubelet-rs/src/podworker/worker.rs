// SPDX-License-Identifier: Apache-2.0
//! `PodWorker` — per-pod state machine.
//!
//! Hand-port of `pkg/kubelet/pod_workers.go` + the relevant slices of
//! `pkg/kubelet/kuberuntime/kuberuntime_manager.go::SyncPod` (v1.36.1).
//!
//! Reconciliation algorithm (per `SyncPod`):
//!   1. Ensure a sandbox exists for the pod UID; if the cached sandbox is
//!      stale (NotReady / removed), recreate it.
//!   2. For each desired container in the spec:
//!      - if no live CRI container with that `metadata.name` exists, or the
//!        existing one is `Exited` and the restart policy permits restart,
//!        `create_container` + `start_container`.
//!   3. For each live CRI container whose name is no longer in the spec,
//!      `stop_container`.
//!
//! On `Terminating` we stop every container.  On `Terminated` we remove
//! every container plus the sandbox.

use std::sync::Arc;

use parking_lot::Mutex;
use thiserror::Error;

use super::types::{PodWorkerState, SyncOutcome, WorkType};
use crate::api::{Container, Pod, RestartPolicy};
use crate::cri::types::{
    Container as CriContainer, ContainerConfig, ContainerMetadata, ContainerState as CriState,
    ImageSpec, PodSandboxConfig, PodSandboxMetadata, PodSandboxState,
};
use crate::cri::{CriClient, CriError};

/// PodWorker error.
#[derive(Debug, Error)]
pub enum PodWorkerError {
    /// CRI failed.
    #[error("CRI error: {0}")]
    Cri(#[from] CriError),
    /// Internal invariant broken.
    #[error("inconsistent state: {0}")]
    Inconsistent(&'static str),
}

/// Per-pod state machine.
pub struct PodWorker {
    cri: Arc<dyn CriClient>,
    state: Mutex<PodWorkerState>,
    /// Cached sandbox CRI ID (None until first successful sandbox creation).
    sandbox_id: Mutex<Option<String>>,
}

impl PodWorker {
    /// Construct a new pod-worker.
    #[must_use]
    pub fn new(cri: Arc<dyn CriClient>) -> Self {
        Self {
            cri,
            state: Mutex::new(PodWorkerState::Idle),
            sandbox_id: Mutex::new(None),
        }
    }

    /// Current state (snapshot).
    pub fn state(&self) -> PodWorkerState {
        *self.state.lock()
    }

    /// Drive one sync iteration.
    pub async fn sync(&self, pod: &Pod, work: WorkType) -> Result<SyncOutcome, PodWorkerError> {
        match work {
            WorkType::Sync => self.do_sync(pod).await,
            WorkType::Terminating => self.do_terminating(pod).await,
            WorkType::Terminated => self.do_terminated(pod).await,
        }
    }

    async fn do_sync(&self, pod: &Pod) -> Result<SyncOutcome, PodWorkerError> {
        // Refuse to revive a worker that's already past terminating.
        let cur = *self.state.lock();
        if matches!(cur, PodWorkerState::Terminated) {
            return Ok(SyncOutcome::empty());
        }

        *self.state.lock() = PodWorkerState::Syncing;
        let mut out = SyncOutcome::empty();

        // 1. Ensure sandbox.
        let sandbox_id = self.ensure_sandbox(pod).await?;
        out.sandbox_id = Some(sandbox_id.clone());

        // 2. Snapshot existing containers for this sandbox.
        let cri_containers = self
            .cri
            .list_containers(Some(crate::cri::types::ContainerFilter {
                pod_sandbox_id: Some(sandbox_id.clone()),
                ..Default::default()
            }))
            .await?;

        // 3. For each desired container, decide what action to take.
        for desired in &pod.spec.containers {
            let action =
                compute_container_action(desired, &cri_containers, pod.spec.restart_policy);
            match action {
                ContainerAction::Skip => {}
                ContainerAction::Create => {
                    let cid = self
                        .cri
                        .create_container(
                            &sandbox_id,
                            container_config(desired),
                            sandbox_config(pod),
                        )
                        .await?;
                    self.cri.start_container(&cid).await?;
                    out.created_containers.push(cid.clone());
                    out.started_containers.push(cid);
                }
            }
        }

        // 4. Kill containers no longer in the spec.
        for c in &cri_containers {
            let still_desired = pod
                .spec
                .containers
                .iter()
                .any(|d| d.name == c.metadata.name);
            if !still_desired && c.state == CriState::Running {
                self.cri.stop_container(&c.id, 30).await?;
                out.killed_containers.push(c.id.clone());
            }
        }

        *self.state.lock() = PodWorkerState::Waiting;
        Ok(out)
    }

    async fn do_terminating(&self, _pod: &Pod) -> Result<SyncOutcome, PodWorkerError> {
        *self.state.lock() = PodWorkerState::Terminating;
        let mut out = SyncOutcome::empty();
        let sandbox_id = match self.sandbox_id.lock().clone() {
            Some(s) => s,
            None => return Ok(out),
        };
        let cri_containers = self
            .cri
            .list_containers(Some(crate::cri::types::ContainerFilter {
                pod_sandbox_id: Some(sandbox_id.clone()),
                ..Default::default()
            }))
            .await?;
        for c in &cri_containers {
            if c.state != CriState::Exited {
                self.cri.stop_container(&c.id, 30).await?;
                out.killed_containers.push(c.id.clone());
            }
        }
        out.sandbox_id = Some(sandbox_id);
        Ok(out)
    }

    async fn do_terminated(&self, _pod: &Pod) -> Result<SyncOutcome, PodWorkerError> {
        let mut out = SyncOutcome::empty();
        let sandbox_id = match self.sandbox_id.lock().clone() {
            Some(s) => s,
            None => {
                *self.state.lock() = PodWorkerState::Terminated;
                return Ok(out);
            }
        };
        // Remove containers (must be Exited; do_terminating already stopped them).
        let cri_containers = self
            .cri
            .list_containers(Some(crate::cri::types::ContainerFilter {
                pod_sandbox_id: Some(sandbox_id.clone()),
                ..Default::default()
            }))
            .await?;
        for c in &cri_containers {
            if c.state == CriState::Running {
                self.cri.stop_container(&c.id, 0).await?;
            }
            self.cri.remove_container(&c.id).await?;
        }
        // Stop & remove the sandbox.
        self.cri.stop_pod_sandbox(&sandbox_id).await?;
        self.cri.remove_pod_sandbox(&sandbox_id).await?;
        out.sandbox_id = Some(sandbox_id);
        *self.sandbox_id.lock() = None;
        *self.state.lock() = PodWorkerState::Terminated;
        Ok(out)
    }

    async fn ensure_sandbox(&self, pod: &Pod) -> Result<String, PodWorkerError> {
        // Fast path: cached id is still valid.
        let cached = self.sandbox_id.lock().clone();
        if let Some(id) = cached {
            if let Ok(st) = self.cri.pod_sandbox_status(&id).await {
                if st.state == PodSandboxState::Ready {
                    return Ok(id);
                }
            }
            // Cached sandbox is gone or not-ready: best-effort cleanup.
            let _ = self.cri.stop_pod_sandbox(&id).await;
            let _ = self.cri.remove_pod_sandbox(&id).await;
            *self.sandbox_id.lock() = None;
        }
        let new_id = self.cri.run_pod_sandbox(sandbox_config(pod)).await?;
        *self.sandbox_id.lock() = Some(new_id.clone());
        Ok(new_id)
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ContainerAction {
    /// Container already running and matches spec — nothing to do.
    Skip,
    /// Need to create + start a fresh CRI container.
    Create,
}

/// Mirrors `pkg/kubelet/kuberuntime/kuberuntime_manager.go::computePodActions`
/// (subset used by Phase 1).
fn compute_container_action(
    desired: &Container,
    cri_containers: &[CriContainer],
    restart_policy: RestartPolicy,
) -> ContainerAction {
    let live_running = cri_containers
        .iter()
        .any(|c| c.metadata.name == desired.name && c.state == CriState::Running);
    if live_running {
        return ContainerAction::Skip;
    }
    let any_exited = cri_containers
        .iter()
        .any(|c| c.metadata.name == desired.name && c.state == CriState::Exited);
    let any_created = cri_containers
        .iter()
        .any(|c| c.metadata.name == desired.name && c.state == CriState::Created);
    if any_created {
        // CRI Created but not Running yet — caller will start it next pass;
        // do not duplicate here.
        return ContainerAction::Skip;
    }
    if any_exited {
        match restart_policy {
            RestartPolicy::Always | RestartPolicy::OnFailure => ContainerAction::Create,
            RestartPolicy::Never => ContainerAction::Skip,
        }
    } else {
        ContainerAction::Create
    }
}

fn sandbox_config(pod: &Pod) -> PodSandboxConfig {
    PodSandboxConfig {
        metadata: PodSandboxMetadata {
            name: pod.metadata.name.clone(),
            uid: pod.metadata.uid.as_str().into(),
            namespace: pod.metadata.namespace.clone(),
            attempt: 0,
        },
        hostname: pod.metadata.name.clone(),
        log_directory: format!(
            "/var/log/pods/{}_{}_{}",
            pod.metadata.namespace,
            pod.metadata.name,
            pod.metadata.uid.as_str()
        ),
        labels: pod.metadata.labels.clone(),
        annotations: pod.metadata.annotations.clone(),
        ..Default::default()
    }
}

fn container_config(c: &Container) -> ContainerConfig {
    ContainerConfig {
        metadata: ContainerMetadata {
            name: c.name.clone(),
            attempt: 0,
        },
        image: ImageSpec {
            image: c.image.clone(),
        },
        command: c.command.clone(),
        args: c.args.clone(),
        ..Default::default()
    }
}
