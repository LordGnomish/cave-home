// SPDX-License-Identifier: Apache-2.0
//! `runtime.v1.RuntimeService` gRPC handler — RED skeleton.
//!
//! Line-by-line port of containerd's `internal/cri/server` Runtime
//! handlers (v2.3.0). Phase 1 fully implements the metadata-level
//! lifecycle (sandbox + container CRUD); the rest return
//! `tonic::Status::unimplemented` AT THE GRPC BOUNDARY ONLY — that is
//! the protocol's defined "not implemented" response, not a Rust
//! `unimplemented!()` panic, and every such RPC is mirrored by an
//! `[[unmapped]]` entry in `parity.manifest.toml`.

use std::pin::Pin;
use std::time::SystemTime;

use async_trait::async_trait;
use futures_core::Stream;
use tonic::{Request, Response, Status};

use crate::cri::container_store::{Container, ContainerStore};
use crate::cri::sandbox_store::{Sandbox, SandboxStore};
use crate::cri::types::{
    ContainerMetadata, ContainerState, SandboxMetadata, SandboxState, SandboxStatus,
};
use crate::runtime_v1 as pb;

/// Translates `SandboxState` to its proto enum value.
const fn sandbox_state_pb(s: SandboxState) -> i32 {
    match s {
        SandboxState::Ready => pb::PodSandboxState::SandboxReady as i32,
        SandboxState::NotReady => pb::PodSandboxState::SandboxNotready as i32,
    }
}

/// Translates `ContainerState` to its proto enum value.
const fn container_state_pb(s: ContainerState) -> i32 {
    match s {
        ContainerState::Created => pb::ContainerState::ContainerCreated as i32,
        ContainerState::Running => pb::ContainerState::ContainerRunning as i32,
        ContainerState::Exited => pb::ContainerState::ContainerExited as i32,
        ContainerState::Unknown => pb::ContainerState::ContainerUnknown as i32,
    }
}

fn unix_ns(t: SystemTime) -> i64 {
    let dur = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    i64::try_from(dur.as_nanos()).unwrap_or(i64::MAX)
}

/// CRI Runtime gRPC server.
#[derive(Debug, Clone)]
pub struct RuntimeServer {
    sandboxes: SandboxStore,
    containers: ContainerStore,
}

impl RuntimeServer {
    /// Builds a runtime server backed by the supplied stores.
    #[must_use]
    pub fn new(sandboxes: SandboxStore, containers: ContainerStore) -> Self {
        Self { sandboxes, containers }
    }

    /// Borrowed sandbox store handle.
    #[must_use]
    pub const fn sandboxes(&self) -> &SandboxStore {
        &self.sandboxes
    }

    /// Borrowed container store handle.
    #[must_use]
    pub const fn containers(&self) -> &ContainerStore {
        &self.containers
    }
}

/// A never-yielding stream — used as the associated type for the
/// streaming RPCs that we return `unimplemented` from.
type EmptyStream<T> =
    Pin<Box<dyn Stream<Item = std::result::Result<T, Status>> + Send + 'static>>;

#[async_trait]
impl pb::runtime_service_server::RuntimeService for RuntimeServer {
    async fn version(
        &self,
        _req: Request<pb::VersionRequest>,
    ) -> Result<Response<pb::VersionResponse>, Status> {
        Ok(Response::new(pb::VersionResponse {
            version: "0.1.0".to_owned(),
            runtime_name: "cave-home-containerd-rs".to_owned(),
            runtime_version: env!("CARGO_PKG_VERSION").to_owned(),
            runtime_api_version: "v1".to_owned(),
        }))
    }

    // ---- PodSandbox lifecycle (FULLY implemented) ------------------

    async fn run_pod_sandbox(
        &self,
        req: Request<pb::RunPodSandboxRequest>,
    ) -> Result<Response<pb::RunPodSandboxResponse>, Status> {
        let req = req.into_inner();
        let cfg = req
            .config
            .ok_or_else(|| Status::invalid_argument("config is required"))?;
        let meta = cfg
            .metadata
            .ok_or_else(|| Status::invalid_argument("config.metadata is required"))?;

        let id = uuid::Uuid::new_v4().to_string().replace('-', "");
        let now = SystemTime::now();
        let sb = Sandbox {
            metadata: SandboxMetadata {
                id: id.clone(),
                name: meta.name,
                uid: meta.uid,
                namespace: meta.namespace,
                runtime_handler: req.runtime_handler,
                net_ns_path: cfg
                    .linux
                    .as_ref()
                    .and_then(|l| l.security_context.as_ref())
                    .map_or_else(String::new, |_| String::new()),
                process_label: String::new(),
                created_at: now,
            },
            status: SandboxStatus { state: SandboxState::Ready, state_changed_at: now },
        };
        self.sandboxes.add(sb).map_err(Status::from)?;
        Ok(Response::new(pb::RunPodSandboxResponse { pod_sandbox_id: id }))
    }

    async fn stop_pod_sandbox(
        &self,
        req: Request<pb::StopPodSandboxRequest>,
    ) -> Result<Response<pb::StopPodSandboxResponse>, Status> {
        let id = req.into_inner().pod_sandbox_id;
        // Idempotent: missing sandbox is not an error per upstream.
        let _ = self.sandboxes.update_status(&id, |st| {
            st.state = SandboxState::NotReady;
            st.state_changed_at = SystemTime::now();
        });
        // Force-stop all containers in the sandbox.
        for c in self.containers.list_for_sandbox(&id) {
            let _ = self.containers.update_status(&c.metadata.id, |s| {
                if s.state == ContainerState::Running {
                    s.state = ContainerState::Exited;
                    s.finished_at = SystemTime::now();
                }
            });
        }
        Ok(Response::new(pb::StopPodSandboxResponse {}))
    }

    async fn remove_pod_sandbox(
        &self,
        req: Request<pb::RemovePodSandboxRequest>,
    ) -> Result<Response<pb::RemovePodSandboxResponse>, Status> {
        let id = req.into_inner().pod_sandbox_id;
        // Refuse if there are still containers — matches upstream
        // server.go RemovePodSandbox precondition.
        if !self.containers.list_for_sandbox(&id).is_empty() {
            return Err(Status::failed_precondition(format!(
                "sandbox {id} still has containers"
            )));
        }
        self.sandboxes.delete(&id);
        Ok(Response::new(pb::RemovePodSandboxResponse {}))
    }

    async fn pod_sandbox_status(
        &self,
        req: Request<pb::PodSandboxStatusRequest>,
    ) -> Result<Response<pb::PodSandboxStatusResponse>, Status> {
        let id = req.into_inner().pod_sandbox_id;
        let sb = self.sandboxes.get(&id).map_err(Status::from)?;
        Ok(Response::new(pb::PodSandboxStatusResponse {
            status: Some(pb::PodSandboxStatus {
                id: sb.metadata.id,
                metadata: Some(pb::PodSandboxMetadata {
                    name: sb.metadata.name,
                    uid: sb.metadata.uid,
                    namespace: sb.metadata.namespace,
                    attempt: 0,
                }),
                state: sandbox_state_pb(sb.status.state),
                created_at: unix_ns(sb.metadata.created_at),
                ..Default::default()
            }),
            info: Default::default(),
            containers_statuses: Vec::new(),
            timestamp: unix_ns(SystemTime::now()),
        }))
    }

    async fn list_pod_sandbox(
        &self,
        _req: Request<pb::ListPodSandboxRequest>,
    ) -> Result<Response<pb::ListPodSandboxResponse>, Status> {
        let items = self
            .sandboxes
            .list()
            .into_iter()
            .map(|sb| pb::PodSandbox {
                id: sb.metadata.id,
                metadata: Some(pb::PodSandboxMetadata {
                    name: sb.metadata.name,
                    uid: sb.metadata.uid,
                    namespace: sb.metadata.namespace,
                    attempt: 0,
                }),
                state: sandbox_state_pb(sb.status.state),
                created_at: unix_ns(sb.metadata.created_at),
                ..Default::default()
            })
            .collect();
        Ok(Response::new(pb::ListPodSandboxResponse { items }))
    }

    type StreamPodSandboxesStream = EmptyStream<pb::StreamPodSandboxesResponse>;
    async fn stream_pod_sandboxes(
        &self,
        _req: Request<pb::StreamPodSandboxesRequest>,
    ) -> Result<Response<Self::StreamPodSandboxesStream>, Status> {
        Err(Status::unimplemented("stream_pod_sandboxes — Phase 1b"))
    }

    // ---- Container lifecycle (FULLY implemented) ------------------

    async fn create_container(
        &self,
        req: Request<pb::CreateContainerRequest>,
    ) -> Result<Response<pb::CreateContainerResponse>, Status> {
        let req = req.into_inner();
        let _sb = self.sandboxes.get(&req.pod_sandbox_id).map_err(Status::from)?;
        let cfg = req
            .config
            .ok_or_else(|| Status::invalid_argument("config is required"))?;
        let meta = cfg
            .metadata
            .ok_or_else(|| Status::invalid_argument("config.metadata is required"))?;
        let image = cfg
            .image
            .ok_or_else(|| Status::invalid_argument("config.image is required"))?;

        let id = uuid::Uuid::new_v4().to_string().replace('-', "");
        let now = SystemTime::now();
        let c = Container {
            metadata: ContainerMetadata {
                id: id.clone(),
                sandbox_id: req.pod_sandbox_id,
                name: meta.name,
                image: image.image,
                runtime_handler: String::new(),
                created_at: now,
            },
            status: Default::default(),
        };
        self.containers.add(c).map_err(Status::from)?;
        Ok(Response::new(pb::CreateContainerResponse { container_id: id }))
    }

    async fn start_container(
        &self,
        req: Request<pb::StartContainerRequest>,
    ) -> Result<Response<pb::StartContainerResponse>, Status> {
        let id = req.into_inner().container_id;
        self.containers
            .update_status(&id, |s| {
                if s.state == ContainerState::Created {
                    s.state = ContainerState::Running;
                    s.started_at = SystemTime::now();
                }
            })
            .map_err(Status::from)?;
        Ok(Response::new(pb::StartContainerResponse {}))
    }

    async fn stop_container(
        &self,
        req: Request<pb::StopContainerRequest>,
    ) -> Result<Response<pb::StopContainerResponse>, Status> {
        let id = req.into_inner().container_id;
        self.containers
            .update_status(&id, |s| {
                if s.state == ContainerState::Running {
                    s.state = ContainerState::Exited;
                    s.finished_at = SystemTime::now();
                }
            })
            .map_err(Status::from)?;
        Ok(Response::new(pb::StopContainerResponse {}))
    }

    async fn remove_container(
        &self,
        req: Request<pb::RemoveContainerRequest>,
    ) -> Result<Response<pb::RemoveContainerResponse>, Status> {
        let id = req.into_inner().container_id;
        self.containers.delete(&id).map_err(Status::from)?;
        Ok(Response::new(pb::RemoveContainerResponse {}))
    }

    async fn list_containers(
        &self,
        _req: Request<pb::ListContainersRequest>,
    ) -> Result<Response<pb::ListContainersResponse>, Status> {
        let containers = self
            .containers
            .list()
            .into_iter()
            .map(|c| pb::Container {
                id: c.metadata.id,
                pod_sandbox_id: c.metadata.sandbox_id,
                metadata: Some(pb::ContainerMetadata {
                    name: c.metadata.name,
                    attempt: 0,
                }),
                image: Some(pb::ImageSpec {
                    image: c.metadata.image,
                    ..Default::default()
                }),
                state: container_state_pb(c.status.state),
                created_at: unix_ns(c.metadata.created_at),
                ..Default::default()
            })
            .collect();
        Ok(Response::new(pb::ListContainersResponse { containers }))
    }

    type StreamContainersStream = EmptyStream<pb::StreamContainersResponse>;
    async fn stream_containers(
        &self,
        _req: Request<pb::StreamContainersRequest>,
    ) -> Result<Response<Self::StreamContainersStream>, Status> {
        Err(Status::unimplemented("stream_containers — Phase 1b"))
    }

    async fn container_status(
        &self,
        req: Request<pb::ContainerStatusRequest>,
    ) -> Result<Response<pb::ContainerStatusResponse>, Status> {
        let id = req.into_inner().container_id;
        let c = self.containers.get(&id).map_err(Status::from)?;
        Ok(Response::new(pb::ContainerStatusResponse {
            status: Some(pb::ContainerStatus {
                id: c.metadata.id,
                metadata: Some(pb::ContainerMetadata {
                    name: c.metadata.name,
                    attempt: 0,
                }),
                state: container_state_pb(c.status.state),
                created_at: unix_ns(c.metadata.created_at),
                started_at: unix_ns(c.status.started_at),
                finished_at: unix_ns(c.status.finished_at),
                exit_code: c.status.exit_code,
                image: Some(pb::ImageSpec { image: c.metadata.image, ..Default::default() }),
                reason: c.status.reason,
                message: c.status.message,
                ..Default::default()
            }),
            info: Default::default(),
        }))
    }

    async fn update_container_resources(
        &self,
        req: Request<pb::UpdateContainerResourcesRequest>,
    ) -> Result<Response<pb::UpdateContainerResourcesResponse>, Status> {
        let id = req.into_inner().container_id;
        // Phase 1: validate the container exists; resource quota
        // application against cgroup v2 is Phase 1b.
        let _ = self.containers.get(&id).map_err(Status::from)?;
        Ok(Response::new(pb::UpdateContainerResourcesResponse {}))
    }

    async fn reopen_container_log(
        &self,
        _req: Request<pb::ReopenContainerLogRequest>,
    ) -> Result<Response<pb::ReopenContainerLogResponse>, Status> {
        Err(Status::unimplemented("reopen_container_log — Phase 1b"))
    }

    // ---- Streaming / exec / stats — gRPC unimplemented (Phase 1b) ---

    async fn exec_sync(
        &self,
        _req: Request<pb::ExecSyncRequest>,
    ) -> Result<Response<pb::ExecSyncResponse>, Status> {
        Err(Status::unimplemented("exec_sync — Phase 1b"))
    }

    async fn exec(
        &self,
        _req: Request<pb::ExecRequest>,
    ) -> Result<Response<pb::ExecResponse>, Status> {
        Err(Status::unimplemented("exec — Phase 1b"))
    }

    async fn attach(
        &self,
        _req: Request<pb::AttachRequest>,
    ) -> Result<Response<pb::AttachResponse>, Status> {
        Err(Status::unimplemented("attach — Phase 1b"))
    }

    async fn port_forward(
        &self,
        _req: Request<pb::PortForwardRequest>,
    ) -> Result<Response<pb::PortForwardResponse>, Status> {
        Err(Status::unimplemented("port_forward — Phase 1b"))
    }

    async fn container_stats(
        &self,
        _req: Request<pb::ContainerStatsRequest>,
    ) -> Result<Response<pb::ContainerStatsResponse>, Status> {
        Err(Status::unimplemented("container_stats — Phase 1b"))
    }

    async fn list_container_stats(
        &self,
        _req: Request<pb::ListContainerStatsRequest>,
    ) -> Result<Response<pb::ListContainerStatsResponse>, Status> {
        Err(Status::unimplemented("list_container_stats — Phase 1b"))
    }

    type StreamContainerStatsStream = EmptyStream<pb::StreamContainerStatsResponse>;
    async fn stream_container_stats(
        &self,
        _req: Request<pb::StreamContainerStatsRequest>,
    ) -> Result<Response<Self::StreamContainerStatsStream>, Status> {
        Err(Status::unimplemented("stream_container_stats — Phase 1b"))
    }

    async fn pod_sandbox_stats(
        &self,
        _req: Request<pb::PodSandboxStatsRequest>,
    ) -> Result<Response<pb::PodSandboxStatsResponse>, Status> {
        Err(Status::unimplemented("pod_sandbox_stats — Phase 1b"))
    }

    async fn list_pod_sandbox_stats(
        &self,
        _req: Request<pb::ListPodSandboxStatsRequest>,
    ) -> Result<Response<pb::ListPodSandboxStatsResponse>, Status> {
        Err(Status::unimplemented("list_pod_sandbox_stats — Phase 1b"))
    }

    type StreamPodSandboxStatsStream = EmptyStream<pb::StreamPodSandboxStatsResponse>;
    async fn stream_pod_sandbox_stats(
        &self,
        _req: Request<pb::StreamPodSandboxStatsRequest>,
    ) -> Result<Response<Self::StreamPodSandboxStatsStream>, Status> {
        Err(Status::unimplemented("stream_pod_sandbox_stats — Phase 1b"))
    }

    async fn update_runtime_config(
        &self,
        _req: Request<pb::UpdateRuntimeConfigRequest>,
    ) -> Result<Response<pb::UpdateRuntimeConfigResponse>, Status> {
        // Phase 1: accept and discard. Real CNI wiring is Phase 1b.
        Ok(Response::new(pb::UpdateRuntimeConfigResponse {}))
    }

    async fn status(
        &self,
        _req: Request<pb::StatusRequest>,
    ) -> Result<Response<pb::StatusResponse>, Status> {
        Ok(Response::new(pb::StatusResponse {
            status: Some(pb::RuntimeStatus {
                conditions: vec![
                    pb::RuntimeCondition {
                        r#type: "RuntimeReady".to_owned(),
                        status: true,
                        reason: String::new(),
                        message: String::new(),
                    },
                    pb::RuntimeCondition {
                        r#type: "NetworkReady".to_owned(),
                        status: false,
                        reason: "PhaseOne".to_owned(),
                        message: "CNI wiring is Phase 1b".to_owned(),
                    },
                ],
            }),
            info: Default::default(),
            runtime_handlers: Vec::new(),
            features: None,
        }))
    }

    async fn checkpoint_container(
        &self,
        _req: Request<pb::CheckpointContainerRequest>,
    ) -> Result<Response<pb::CheckpointContainerResponse>, Status> {
        Err(Status::unimplemented("checkpoint_container — Phase 1b"))
    }

    type GetContainerEventsStream = EmptyStream<pb::ContainerEventResponse>;
    async fn get_container_events(
        &self,
        _req: Request<pb::GetEventsRequest>,
    ) -> Result<Response<Self::GetContainerEventsStream>, Status> {
        Err(Status::unimplemented("get_container_events — Phase 1b"))
    }

    async fn list_metric_descriptors(
        &self,
        _req: Request<pb::ListMetricDescriptorsRequest>,
    ) -> Result<Response<pb::ListMetricDescriptorsResponse>, Status> {
        Err(Status::unimplemented("list_metric_descriptors — Phase 1b"))
    }

    async fn list_pod_sandbox_metrics(
        &self,
        _req: Request<pb::ListPodSandboxMetricsRequest>,
    ) -> Result<Response<pb::ListPodSandboxMetricsResponse>, Status> {
        Err(Status::unimplemented("list_pod_sandbox_metrics — Phase 1b"))
    }

    type StreamPodSandboxMetricsStream = EmptyStream<pb::StreamPodSandboxMetricsResponse>;
    async fn stream_pod_sandbox_metrics(
        &self,
        _req: Request<pb::StreamPodSandboxMetricsRequest>,
    ) -> Result<Response<Self::StreamPodSandboxMetricsStream>, Status> {
        Err(Status::unimplemented("stream_pod_sandbox_metrics — Phase 1b"))
    }

    async fn runtime_config(
        &self,
        _req: Request<pb::RuntimeConfigRequest>,
    ) -> Result<Response<pb::RuntimeConfigResponse>, Status> {
        Ok(Response::new(pb::RuntimeConfigResponse {
            linux: None,
        }))
    }

    async fn update_pod_sandbox_resources(
        &self,
        _req: Request<pb::UpdatePodSandboxResourcesRequest>,
    ) -> Result<Response<pb::UpdatePodSandboxResourcesResponse>, Status> {
        Err(Status::unimplemented("update_pod_sandbox_resources — Phase 1b"))
    }
}
