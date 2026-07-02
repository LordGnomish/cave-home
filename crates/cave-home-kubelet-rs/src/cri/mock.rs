// SPDX-License-Identifier: Apache-2.0
//! In-memory `MockCriClient` — deterministic, single-process state machine
//! used by every kubelet sub-system test. Hand-port of the in-memory fakes
//! living in `pkg/kubelet/kuberuntime/fake_kuberuntime_manager.go` and
//! `pkg/kubelet/container/testing/runtime_mock.go`.
//!
//! Real gRPC wiring is `[[unmapped]]` Phase 1b — it is a workspace-level
//! integration concern, not a kubelet concern.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use parking_lot::Mutex;

use super::client::{CriClient, CriError, CriResult};
use super::types::{
    Container, ContainerConfig, ContainerFilter, ContainerState, ContainerStatus, FilesystemUsage,
    Image, ImageSpec, PodSandbox, PodSandboxConfig, PodSandboxFilter, PodSandboxState,
    PodSandboxStatus,
};

/// Per-sandbox record kept by the mock.
#[derive(Clone, Debug)]
struct SandboxRecord {
    sandbox: PodSandbox,
    /// Stored for parity with the real CRI client which echoes the
    /// configured cgroup parent / namespace options back through
    /// `pod_sandbox_status`. Phase 1 leaves it untouched.
    #[allow(dead_code)]
    cfg: PodSandboxConfig,
}

/// Per-container record kept by the mock.
#[derive(Clone, Debug)]
struct ContainerRecord {
    container: Container,
    started_at: i64,
    finished_at: i64,
    exit_code: i32,
}

#[derive(Default)]
struct State {
    sandboxes: HashMap<String, SandboxRecord>,
    containers: HashMap<String, ContainerRecord>,
    images: HashMap<String, Image>,
}

/// Deterministic in-memory CRI mock.
pub struct MockCriClient {
    state: Mutex<State>,
    seq: AtomicU64,
    /// Synthetic clock — incremented by 1 on every event so tests stay
    /// deterministic.
    clock: AtomicU64,
}

impl Default for MockCriClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCriClient {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State::default()),
            seq: AtomicU64::new(1),
            clock: AtomicU64::new(1),
        }
    }

    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.seq.fetch_add(1, Ordering::SeqCst))
    }

    fn tick(&self) -> i64 {
        // i64 cast is safe: we will not run 2^63 events in a test.
        self.clock.fetch_add(1, Ordering::SeqCst) as i64
    }
}

#[async_trait]
impl CriClient for MockCriClient {
    async fn version(&self) -> CriResult<String> {
        Ok("cave-home-kubelet-rs/v1.36.1-mock".into())
    }

    async fn run_pod_sandbox(&self, cfg: PodSandboxConfig) -> CriResult<String> {
        let id = self.next_id("sandbox");
        let created = self.tick();
        let sandbox = PodSandbox {
            id: id.clone(),
            metadata: cfg.metadata.clone(),
            state: PodSandboxState::Ready,
            created_at: created,
            labels: cfg.labels.clone(),
        };
        self.state
            .lock()
            .sandboxes
            .insert(id.clone(), SandboxRecord { sandbox, cfg });
        Ok(id)
    }

    async fn stop_pod_sandbox(&self, sandbox_id: &str) -> CriResult<()> {
        let mut s = self.state.lock();
        let rec = s
            .sandboxes
            .get_mut(sandbox_id)
            .ok_or_else(|| CriError::NotFound(format!("pod sandbox {sandbox_id}")))?;
        rec.sandbox.state = PodSandboxState::NotReady;
        // Stop every container belonging to this sandbox.
        for c in s.containers.values_mut() {
            if c.container.pod_sandbox_id == sandbox_id
                && c.container.state == ContainerState::Running
            {
                c.container.state = ContainerState::Exited;
                c.finished_at = self.clock.fetch_add(1, Ordering::SeqCst) as i64;
            }
        }
        Ok(())
    }

    async fn remove_pod_sandbox(&self, sandbox_id: &str) -> CriResult<()> {
        let mut s = self.state.lock();
        if !s.sandboxes.contains_key(sandbox_id) {
            return Err(CriError::NotFound(format!("pod sandbox {sandbox_id}")));
        }
        if let Some(rec) = s.sandboxes.get(sandbox_id) {
            if rec.sandbox.state == PodSandboxState::Ready {
                return Err(CriError::InvalidState(format!(
                    "pod sandbox {sandbox_id} must be stopped before removal"
                )));
            }
        }
        s.sandboxes.remove(sandbox_id);
        // Remove every container belonging to this sandbox too.
        s.containers
            .retain(|_, c| c.container.pod_sandbox_id != sandbox_id);
        Ok(())
    }

    async fn pod_sandbox_status(&self, sandbox_id: &str) -> CriResult<PodSandboxStatus> {
        let s = self.state.lock();
        let rec = s
            .sandboxes
            .get(sandbox_id)
            .ok_or_else(|| CriError::NotFound(format!("pod sandbox {sandbox_id}")))?;
        Ok(PodSandboxStatus {
            id: rec.sandbox.id.clone(),
            metadata: rec.sandbox.metadata.clone(),
            state: rec.sandbox.state,
            created_at: rec.sandbox.created_at,
        })
    }

    async fn list_pod_sandbox(
        &self,
        filter: Option<PodSandboxFilter>,
    ) -> CriResult<Vec<PodSandbox>> {
        let s = self.state.lock();
        let mut out: Vec<PodSandbox> = s.sandboxes.values().map(|r| r.sandbox.clone()).collect();
        if let Some(f) = filter {
            if let Some(id) = f.id {
                out.retain(|sb| sb.id == id);
            }
            if let Some(state) = f.state {
                out.retain(|sb| sb.state == state);
            }
        }
        // Stable order: sort by created_at then id.
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    async fn create_container(
        &self,
        sandbox_id: &str,
        cfg: ContainerConfig,
        _sandbox_cfg: PodSandboxConfig,
    ) -> CriResult<String> {
        {
            let s = self.state.lock();
            if !s.sandboxes.contains_key(sandbox_id) {
                return Err(CriError::NotFound(format!("pod sandbox {sandbox_id}")));
            }
        }
        let id = self.next_id("container");
        let created = self.tick();
        let container = Container {
            id: id.clone(),
            pod_sandbox_id: sandbox_id.into(),
            metadata: cfg.metadata.clone(),
            image: cfg.image.clone(),
            state: ContainerState::Created,
            created_at: created,
            labels: cfg.labels.clone(),
        };
        self.state.lock().containers.insert(
            id.clone(),
            ContainerRecord {
                container,
                started_at: 0,
                finished_at: 0,
                exit_code: 0,
            },
        );
        Ok(id)
    }

    async fn start_container(&self, container_id: &str) -> CriResult<()> {
        let started = self.tick();
        let mut s = self.state.lock();
        let rec = s
            .containers
            .get_mut(container_id)
            .ok_or_else(|| CriError::NotFound(format!("container {container_id}")))?;
        if rec.container.state != ContainerState::Created {
            return Err(CriError::InvalidState(format!(
                "container {container_id} is not in Created state"
            )));
        }
        rec.container.state = ContainerState::Running;
        rec.started_at = started;
        Ok(())
    }

    async fn stop_container(&self, container_id: &str, _timeout_seconds: i64) -> CriResult<()> {
        let finished = self.tick();
        let mut s = self.state.lock();
        let rec = s
            .containers
            .get_mut(container_id)
            .ok_or_else(|| CriError::NotFound(format!("container {container_id}")))?;
        if rec.container.state == ContainerState::Exited {
            return Ok(()); // idempotent
        }
        rec.container.state = ContainerState::Exited;
        rec.finished_at = finished;
        Ok(())
    }

    async fn remove_container(&self, container_id: &str) -> CriResult<()> {
        let mut s = self.state.lock();
        let rec = s
            .containers
            .get(container_id)
            .ok_or_else(|| CriError::NotFound(format!("container {container_id}")))?;
        if rec.container.state == ContainerState::Running {
            return Err(CriError::InvalidState(format!(
                "container {container_id} must be stopped before removal"
            )));
        }
        s.containers.remove(container_id);
        Ok(())
    }

    async fn container_status(&self, container_id: &str) -> CriResult<ContainerStatus> {
        let s = self.state.lock();
        let rec = s
            .containers
            .get(container_id)
            .ok_or_else(|| CriError::NotFound(format!("container {container_id}")))?;
        Ok(ContainerStatus {
            id: rec.container.id.clone(),
            metadata: rec.container.metadata.clone(),
            state: rec.container.state,
            created_at: rec.container.created_at,
            started_at: rec.started_at,
            finished_at: rec.finished_at,
            exit_code: rec.exit_code,
            image: rec.container.image.clone(),
            reason: String::new(),
            message: String::new(),
        })
    }

    async fn list_containers(&self, filter: Option<ContainerFilter>) -> CriResult<Vec<Container>> {
        let s = self.state.lock();
        let mut out: Vec<Container> = s.containers.values().map(|r| r.container.clone()).collect();
        if let Some(f) = filter {
            if let Some(id) = f.id {
                out.retain(|c| c.id == id);
            }
            if let Some(sid) = f.pod_sandbox_id {
                out.retain(|c| c.pod_sandbox_id == sid);
            }
            if let Some(state) = f.state {
                out.retain(|c| c.state == state);
            }
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    async fn pull_image(&self, image: ImageSpec) -> CriResult<String> {
        let id = self.next_id("image");
        let img = Image {
            id: id.clone(),
            repo_tags: vec![image.image.clone()],
            size_bytes: 0,
        };
        self.state.lock().images.insert(image.image, img);
        Ok(id)
    }

    async fn image_status(&self, image: ImageSpec) -> CriResult<Option<Image>> {
        Ok(self.state.lock().images.get(&image.image).cloned())
    }

    async fn list_images(&self, filter: Option<ImageSpec>) -> CriResult<Vec<Image>> {
        let want = filter.map(|s| s.image);
        let mut out: Vec<Image> = {
            let s = self.state.lock();
            s.images
                .values()
                .filter(|i| {
                    want.as_ref()
                        .is_none_or(|w| i.id == *w || i.repo_tags.contains(w))
                })
                .cloned()
                .collect()
        };
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    async fn remove_image(&self, image: ImageSpec) -> CriResult<()> {
        // Idempotent: keyed by repo-tag (the pull key) or image id.
        self.state
            .lock()
            .images
            .retain(|key, img| *key != image.image && img.id != image.image);
        Ok(())
    }

    async fn image_fs_info(&self) -> CriResult<Vec<FilesystemUsage>> {
        let (used_bytes, inodes_used) = {
            let s = self.state.lock();
            (
                s.images.values().map(|i| i.size_bytes.max(1)).sum(),
                s.images.len() as u64,
            )
        };
        Ok(vec![FilesystemUsage {
            timestamp: i64::try_from(self.clock.load(Ordering::SeqCst)).unwrap_or(i64::MAX),
            mountpoint: "/var/lib/cave-home/images".into(),
            used_bytes,
            inodes_used,
        }])
    }
}
