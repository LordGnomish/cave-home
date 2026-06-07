// SPDX-License-Identifier: Apache-2.0
//! Shared test support: a stateful in-process CRI runtime served over a real
//! Unix-domain socket (same transport containerd exposes), so the
//! `RemoteCriClient` integration tests exercise genuine gRPC round-trips —
//! protobuf encode, HTTP/2 over a UDS, decode — not an in-memory shortcut.
#![cfg(feature = "remote-cri")]
// Each test binary that `mod common;`s this file only uses part of it.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use cave_home_kubelet_rs::cri::remote::proto;
use parking_lot::Mutex;
use proto::image_service_server::{ImageService, ImageServiceServer};
use proto::runtime_service_server::{RuntimeService, RuntimeServiceServer};
use tonic::{Request, Response, Status};

type RpcResult<T> = Result<Response<T>, Status>;

#[derive(Default)]
struct State {
    sandboxes: HashMap<String, proto::PodSandboxStatus>,
    containers: HashMap<String, proto::ContainerStatus>,
    container_sandbox: HashMap<String, String>,
    images: HashMap<String, proto::Image>,
    seq: u64,
}

impl State {
    fn next_id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        format!("{prefix}-{}", self.seq)
    }
}

/// A minimal but faithful in-memory CRI runtime used as a gRPC server double.
#[derive(Clone, Default)]
pub struct MockCriRuntime {
    state: Arc<Mutex<State>>,
}

impl MockCriRuntime {
    /// Number of sandboxes currently tracked (test introspection).
    #[must_use]
    pub fn sandbox_count(&self) -> usize {
        self.state.lock().sandboxes.len()
    }

    /// Number of containers currently tracked (test introspection).
    #[must_use]
    pub fn container_count(&self) -> usize {
        self.state.lock().containers.len()
    }
}

fn not_found(what: &str, id: &str) -> Status {
    Status::not_found(format!("{what} {id}"))
}

#[tonic::async_trait]
impl RuntimeService for MockCriRuntime {
    async fn version(&self, _req: Request<proto::VersionRequest>) -> RpcResult<proto::VersionResponse> {
        Ok(Response::new(proto::VersionResponse {
            version: "v1".into(),
            runtime_name: "mock-containerd".into(),
            runtime_version: "1.7.0-mock".into(),
            runtime_api_version: "v1".into(),
        }))
    }

    async fn run_pod_sandbox(
        &self,
        req: Request<proto::RunPodSandboxRequest>,
    ) -> RpcResult<proto::RunPodSandboxResponse> {
        let cfg = req.into_inner().config.unwrap_or_default();
        let mut st = self.state.lock();
        let id = st.next_id("sb");
        let created = 1_000_000_000 + st.seq as i64;
        st.sandboxes.insert(
            id.clone(),
            proto::PodSandboxStatus {
                id: id.clone(),
                metadata: cfg.metadata,
                state: proto::PodSandboxState::SandboxReady as i32,
                created_at: created,
                labels: cfg.labels,
                ..Default::default()
            },
        );
        Ok(Response::new(proto::RunPodSandboxResponse { pod_sandbox_id: id }))
    }

    async fn stop_pod_sandbox(
        &self,
        req: Request<proto::StopPodSandboxRequest>,
    ) -> RpcResult<proto::StopPodSandboxResponse> {
        let id = req.into_inner().pod_sandbox_id;
        let mut st = self.state.lock();
        let sb = st.sandboxes.get_mut(&id).ok_or_else(|| not_found("pod sandbox", &id))?;
        sb.state = proto::PodSandboxState::SandboxNotready as i32;
        Ok(Response::new(proto::StopPodSandboxResponse {}))
    }

    async fn remove_pod_sandbox(
        &self,
        req: Request<proto::RemovePodSandboxRequest>,
    ) -> RpcResult<proto::RemovePodSandboxResponse> {
        let id = req.into_inner().pod_sandbox_id;
        self.state.lock().sandboxes.remove(&id);
        Ok(Response::new(proto::RemovePodSandboxResponse {}))
    }

    async fn pod_sandbox_status(
        &self,
        req: Request<proto::PodSandboxStatusRequest>,
    ) -> RpcResult<proto::PodSandboxStatusResponse> {
        let id = req.into_inner().pod_sandbox_id;
        let st = self.state.lock();
        let status = st.sandboxes.get(&id).cloned().ok_or_else(|| not_found("pod sandbox", &id))?;
        Ok(Response::new(proto::PodSandboxStatusResponse {
            status: Some(status),
            ..Default::default()
        }))
    }

    async fn list_pod_sandbox(
        &self,
        req: Request<proto::ListPodSandboxRequest>,
    ) -> RpcResult<proto::ListPodSandboxResponse> {
        let filter = req.into_inner().filter.unwrap_or_default();
        let want_state = filter.state.map(|s| s.state);
        let st = self.state.lock();
        let items = st
            .sandboxes
            .values()
            .filter(|s| filter.id.is_empty() || s.id == filter.id)
            .filter(|s| want_state.is_none_or(|ws| ws == s.state))
            .map(|s| proto::PodSandbox {
                id: s.id.clone(),
                metadata: s.metadata.clone(),
                state: s.state,
                created_at: s.created_at,
                labels: s.labels.clone(),
                ..Default::default()
            })
            .collect();
        Ok(Response::new(proto::ListPodSandboxResponse { items }))
    }

    async fn create_container(
        &self,
        req: Request<proto::CreateContainerRequest>,
    ) -> RpcResult<proto::CreateContainerResponse> {
        let r = req.into_inner();
        let cfg = r.config.unwrap_or_default();
        let mut st = self.state.lock();
        if !st.sandboxes.contains_key(&r.pod_sandbox_id) {
            return Err(not_found("pod sandbox", &r.pod_sandbox_id));
        }
        let id = st.next_id("c");
        let created = 2_000_000_000 + st.seq as i64;
        st.containers.insert(
            id.clone(),
            proto::ContainerStatus {
                id: id.clone(),
                metadata: cfg.metadata,
                state: proto::ContainerState::ContainerCreated as i32,
                created_at: created,
                image: cfg.image,
                labels: cfg.labels,
                ..Default::default()
            },
        );
        st.container_sandbox.insert(id.clone(), r.pod_sandbox_id);
        Ok(Response::new(proto::CreateContainerResponse { container_id: id }))
    }

    async fn start_container(
        &self,
        req: Request<proto::StartContainerRequest>,
    ) -> RpcResult<proto::StartContainerResponse> {
        let id = req.into_inner().container_id;
        let mut st = self.state.lock();
        let c = st.containers.get_mut(&id).ok_or_else(|| not_found("container", &id))?;
        c.state = proto::ContainerState::ContainerRunning as i32;
        c.started_at = c.created_at + 1;
        Ok(Response::new(proto::StartContainerResponse {}))
    }

    async fn stop_container(
        &self,
        req: Request<proto::StopContainerRequest>,
    ) -> RpcResult<proto::StopContainerResponse> {
        let id = req.into_inner().container_id;
        let mut st = self.state.lock();
        let c = st.containers.get_mut(&id).ok_or_else(|| not_found("container", &id))?;
        c.state = proto::ContainerState::ContainerExited as i32;
        c.finished_at = c.started_at + 1;
        c.exit_code = 0;
        Ok(Response::new(proto::StopContainerResponse {}))
    }

    async fn remove_container(
        &self,
        req: Request<proto::RemoveContainerRequest>,
    ) -> RpcResult<proto::RemoveContainerResponse> {
        let id = req.into_inner().container_id;
        let mut st = self.state.lock();
        st.containers.remove(&id);
        st.container_sandbox.remove(&id);
        Ok(Response::new(proto::RemoveContainerResponse {}))
    }

    async fn container_status(
        &self,
        req: Request<proto::ContainerStatusRequest>,
    ) -> RpcResult<proto::ContainerStatusResponse> {
        let id = req.into_inner().container_id;
        let st = self.state.lock();
        let status = st.containers.get(&id).cloned().ok_or_else(|| not_found("container", &id))?;
        Ok(Response::new(proto::ContainerStatusResponse {
            status: Some(status),
            ..Default::default()
        }))
    }

    async fn list_containers(
        &self,
        req: Request<proto::ListContainersRequest>,
    ) -> RpcResult<proto::ListContainersResponse> {
        let filter = req.into_inner().filter.unwrap_or_default();
        let want_state = filter.state.map(|s| s.state);
        let st = self.state.lock();
        let containers = st
            .containers
            .values()
            .filter(|c| filter.id.is_empty() || c.id == filter.id)
            .filter(|c| {
                filter.pod_sandbox_id.is_empty()
                    || st.container_sandbox.get(&c.id).map(String::as_str) == Some(&filter.pod_sandbox_id)
            })
            .filter(|c| want_state.is_none_or(|ws| ws == c.state))
            .map(|c| proto::Container {
                id: c.id.clone(),
                pod_sandbox_id: st.container_sandbox.get(&c.id).cloned().unwrap_or_default(),
                metadata: c.metadata.clone(),
                image: c.image.clone(),
                state: c.state,
                created_at: c.created_at,
                labels: c.labels.clone(),
                ..Default::default()
            })
            .collect();
        Ok(Response::new(proto::ListContainersResponse { containers }))
    }

    // --- endpoints the Phase-1 client does not drive: honest unimplemented ---
    async fn update_container_resources(
        &self,
        _r: Request<proto::UpdateContainerResourcesRequest>,
    ) -> RpcResult<proto::UpdateContainerResourcesResponse> {
        Err(Status::unimplemented("mock: update_container_resources"))
    }
    async fn reopen_container_log(
        &self,
        _r: Request<proto::ReopenContainerLogRequest>,
    ) -> RpcResult<proto::ReopenContainerLogResponse> {
        Err(Status::unimplemented("mock: reopen_container_log"))
    }
    async fn exec_sync(&self, _r: Request<proto::ExecSyncRequest>) -> RpcResult<proto::ExecSyncResponse> {
        Err(Status::unimplemented("mock: exec_sync"))
    }
    async fn exec(&self, r: Request<proto::ExecRequest>) -> RpcResult<proto::ExecResponse> {
        let id = r.into_inner().container_id;
        Ok(Response::new(proto::ExecResponse {
            url: format!("http://stream.local/exec/{id}"),
        }))
    }
    async fn attach(&self, r: Request<proto::AttachRequest>) -> RpcResult<proto::AttachResponse> {
        let id = r.into_inner().container_id;
        Ok(Response::new(proto::AttachResponse {
            url: format!("http://stream.local/attach/{id}"),
        }))
    }
    async fn port_forward(
        &self,
        r: Request<proto::PortForwardRequest>,
    ) -> RpcResult<proto::PortForwardResponse> {
        let id = r.into_inner().pod_sandbox_id;
        Ok(Response::new(proto::PortForwardResponse {
            url: format!("http://stream.local/portforward/{id}"),
        }))
    }
    async fn container_stats(
        &self,
        _r: Request<proto::ContainerStatsRequest>,
    ) -> RpcResult<proto::ContainerStatsResponse> {
        Err(Status::unimplemented("mock: container_stats"))
    }
    async fn list_container_stats(
        &self,
        _r: Request<proto::ListContainerStatsRequest>,
    ) -> RpcResult<proto::ListContainerStatsResponse> {
        Err(Status::unimplemented("mock: list_container_stats"))
    }
    async fn pod_sandbox_stats(
        &self,
        _r: Request<proto::PodSandboxStatsRequest>,
    ) -> RpcResult<proto::PodSandboxStatsResponse> {
        Err(Status::unimplemented("mock: pod_sandbox_stats"))
    }
    async fn list_pod_sandbox_stats(
        &self,
        _r: Request<proto::ListPodSandboxStatsRequest>,
    ) -> RpcResult<proto::ListPodSandboxStatsResponse> {
        Err(Status::unimplemented("mock: list_pod_sandbox_stats"))
    }
    async fn update_runtime_config(
        &self,
        _r: Request<proto::UpdateRuntimeConfigRequest>,
    ) -> RpcResult<proto::UpdateRuntimeConfigResponse> {
        Err(Status::unimplemented("mock: update_runtime_config"))
    }
    async fn status(&self, _r: Request<proto::StatusRequest>) -> RpcResult<proto::StatusResponse> {
        Err(Status::unimplemented("mock: status"))
    }
    async fn checkpoint_container(
        &self,
        _r: Request<proto::CheckpointContainerRequest>,
    ) -> RpcResult<proto::CheckpointContainerResponse> {
        Err(Status::unimplemented("mock: checkpoint_container"))
    }

    type GetContainerEventsStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<proto::ContainerEventResponse, Status>> + Send>>;
    async fn get_container_events(
        &self,
        _r: Request<proto::GetEventsRequest>,
    ) -> RpcResult<Self::GetContainerEventsStream> {
        Err(Status::unimplemented("mock: get_container_events"))
    }

    async fn list_metric_descriptors(
        &self,
        _r: Request<proto::ListMetricDescriptorsRequest>,
    ) -> RpcResult<proto::ListMetricDescriptorsResponse> {
        Err(Status::unimplemented("mock: list_metric_descriptors"))
    }
    async fn list_pod_sandbox_metrics(
        &self,
        _r: Request<proto::ListPodSandboxMetricsRequest>,
    ) -> RpcResult<proto::ListPodSandboxMetricsResponse> {
        Err(Status::unimplemented("mock: list_pod_sandbox_metrics"))
    }
    async fn runtime_config(
        &self,
        _r: Request<proto::RuntimeConfigRequest>,
    ) -> RpcResult<proto::RuntimeConfigResponse> {
        Err(Status::unimplemented("mock: runtime_config"))
    }
}

#[tonic::async_trait]
impl ImageService for MockCriRuntime {
    async fn list_images(
        &self,
        req: Request<proto::ListImagesRequest>,
    ) -> RpcResult<proto::ListImagesResponse> {
        // Honour the ImageFilter the same way a real runtime does: match the
        // requested spec against image id or any repo-tag.
        let want = req
            .into_inner()
            .filter
            .and_then(|f| f.image)
            .map(|s| s.image)
            .filter(|s| !s.is_empty());
        let st = self.state.lock();
        let images = st
            .images
            .values()
            .filter(|i| match &want {
                Some(w) => i.id == *w || i.repo_tags.iter().any(|t| t == w),
                None => true,
            })
            .cloned()
            .collect();
        Ok(Response::new(proto::ListImagesResponse { images }))
    }

    async fn image_status(
        &self,
        req: Request<proto::ImageStatusRequest>,
    ) -> RpcResult<proto::ImageStatusResponse> {
        let spec = req.into_inner().image.unwrap_or_default();
        let st = self.state.lock();
        let image = st
            .images
            .values()
            .find(|i| i.id == spec.image || i.repo_tags.iter().any(|t| *t == spec.image))
            .cloned();
        Ok(Response::new(proto::ImageStatusResponse {
            image,
            ..Default::default()
        }))
    }

    async fn pull_image(
        &self,
        req: Request<proto::PullImageRequest>,
    ) -> RpcResult<proto::PullImageResponse> {
        let spec = req.into_inner().image.unwrap_or_default();
        let mut st = self.state.lock();
        let id = format!("sha256:mock-{}", spec.image);
        st.images.insert(
            id.clone(),
            proto::Image {
                id: id.clone(),
                repo_tags: vec![spec.image],
                size: 1,
                ..Default::default()
            },
        );
        Ok(Response::new(proto::PullImageResponse { image_ref: id }))
    }

    async fn remove_image(
        &self,
        req: Request<proto::RemoveImageRequest>,
    ) -> RpcResult<proto::RemoveImageResponse> {
        let spec = req.into_inner().image.unwrap_or_default();
        let mut st = self.state.lock();
        st.images.retain(|id, i| *id != spec.image && i.repo_tags.iter().all(|t| *t != spec.image));
        Ok(Response::new(proto::RemoveImageResponse {}))
    }

    async fn image_fs_info(
        &self,
        _r: Request<proto::ImageFsInfoRequest>,
    ) -> RpcResult<proto::ImageFsInfoResponse> {
        let st = self.state.lock();
        let used: u64 = st.images.values().map(|i| i.size.max(1)).sum();
        Ok(Response::new(proto::ImageFsInfoResponse {
            image_filesystems: vec![proto::FilesystemUsage {
                timestamp: 1_700_000_000,
                fs_id: Some(proto::FilesystemIdentifier {
                    mountpoint: "/var/lib/containerd".into(),
                }),
                used_bytes: Some(proto::UInt64Value { value: used.max(1) }),
                inodes_used: Some(proto::UInt64Value {
                    value: st.images.len() as u64,
                }),
            }],
            container_filesystems: vec![],
        }))
    }
}

/// A running mock server bound to a Unix socket. Dropping it shuts the server
/// down and deletes the socket's tempdir.
pub struct MockServerHandle {
    /// Filesystem path of the bound CRI Unix socket.
    pub socket_path: PathBuf,
    /// Live handle to the backing runtime for state assertions.
    pub runtime: MockCriRuntime,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    _tmp: tempfile::TempDir,
}

impl Drop for MockServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Start the mock CRI server on a fresh Unix socket and return a handle.
///
/// The listener is bound *before* the accept task is spawned, so a client may
/// connect immediately without racing server startup.
#[must_use]
pub async fn start_mock_cri_server() -> MockServerHandle {
    let tmp = tempfile::tempdir().expect("tempdir");
    let socket_path = tmp.path().join("cri.sock");
    let runtime = MockCriRuntime::default();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    let uds = tokio::net::UnixListener::bind(&socket_path).expect("bind uds");
    let incoming = tokio_stream::wrappers::UnixListenerStream::new(uds);

    let rt = runtime.clone();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(RuntimeServiceServer::new(rt.clone()))
            .add_service(ImageServiceServer::new(rt))
            .serve_with_incoming_shutdown(incoming, async {
                let _ = rx.await;
            })
            .await
            .expect("mock server");
    });

    MockServerHandle {
        socket_path,
        runtime,
        shutdown: Some(tx),
        _tmp: tmp,
    }
}
