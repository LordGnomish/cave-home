// SPDX-License-Identifier: Apache-2.0
//! [`RemoteCriClient`] — the gRPC implementation of [`CriClient`].
//!
//! Line-by-line analogue of `k8s.io/kubernetes/pkg/kubelet/cri/remote`
//! (`remote_runtime.go` + `remote_image.go`): it holds a connected
//! `RuntimeServiceClient` and `ImageServiceClient` and turns each [`CriClient`]
//! call into a gRPC round-trip, marshalling via [`super::conv`] and mapping
//! errors via [`super::error`].

use std::path::Path;

use async_trait::async_trait;
use tonic::transport::{Channel, Endpoint, Uri};

use super::error::{status_to_cri_error, transport_to_cri_error};
use super::proto;
use crate::cri::client::{CriClient, CriError, CriResult};
use crate::cri::types as t;

use proto::image_service_client::ImageServiceClient;
use proto::runtime_service_client::RuntimeServiceClient;

/// Kubelet CRI API version string sent in `Version` requests.
const KUBE_RUNTIME_API_VERSION: &str = "v1";

/// gRPC-backed CRI client. Cheap to clone (clones share the HTTP/2 channel).
#[derive(Clone, Debug)]
pub struct RemoteCriClient {
    runtime: RuntimeServiceClient<Channel>,
    image: ImageServiceClient<Channel>,
}

impl RemoteCriClient {
    /// Wrap an already-connected channel (e.g. a custom-configured `Endpoint`).
    #[must_use]
    pub fn from_channel(channel: Channel) -> Self {
        Self {
            runtime: RuntimeServiceClient::new(channel.clone()),
            image: ImageServiceClient::new(channel),
        }
    }

    /// Connect over TCP to `endpoint` (e.g. `http://127.0.0.1:8080`).
    ///
    /// # Errors
    /// Returns [`CriError::Transport`] if the endpoint is malformed or the
    /// channel cannot be established.
    pub async fn connect_tcp(endpoint: impl Into<String>) -> CriResult<Self> {
        let channel = Endpoint::try_from(endpoint.into())
            .map_err(|e| CriError::Transport(e.to_string()))?
            .connect()
            .await
            .map_err(|e| transport_to_cri_error(&e))?;
        Ok(Self::from_channel(channel))
    }

    /// Connect over a Unix-domain socket — the transport containerd's CRI
    /// endpoint listens on (e.g. `/run/containerd/containerd.sock`).
    ///
    /// # Errors
    /// Returns [`CriError::Transport`] if the socket cannot be dialed.
    pub async fn connect_uds(path: impl AsRef<Path>) -> CriResult<Self> {
        let path = path.as_ref().to_path_buf();
        // The authority is unused by the custom connector but must parse.
        let channel = Endpoint::try_from("http://[::]:50051")
            .map_err(|e| CriError::Transport(e.to_string()))?
            .connect_with_connector(tower::service_fn(move |_: Uri| {
                let path = path.clone();
                async move {
                    let stream = tokio::net::UnixStream::connect(path).await?;
                    Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(stream))
                }
            }))
            .await
            .map_err(|e| transport_to_cri_error(&e))?;
        Ok(Self::from_channel(channel))
    }
}

#[async_trait]
impl CriClient for RemoteCriClient {
    async fn version(&self) -> CriResult<String> {
        let req = proto::VersionRequest {
            version: KUBE_RUNTIME_API_VERSION.to_owned(),
        };
        let resp = self
            .runtime
            .clone()
            .version(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(resp.into_inner().runtime_version)
    }

    async fn run_pod_sandbox(&self, cfg: t::PodSandboxConfig) -> CriResult<String> {
        let req = proto::RunPodSandboxRequest {
            config: Some(cfg.into()),
            runtime_handler: String::new(),
        };
        let resp = self
            .runtime
            .clone()
            .run_pod_sandbox(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(resp.into_inner().pod_sandbox_id)
    }

    async fn stop_pod_sandbox(&self, sandbox_id: &str) -> CriResult<()> {
        let req = proto::StopPodSandboxRequest {
            pod_sandbox_id: sandbox_id.to_owned(),
        };
        self.runtime
            .clone()
            .stop_pod_sandbox(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(())
    }

    async fn remove_pod_sandbox(&self, sandbox_id: &str) -> CriResult<()> {
        let req = proto::RemovePodSandboxRequest {
            pod_sandbox_id: sandbox_id.to_owned(),
        };
        self.runtime
            .clone()
            .remove_pod_sandbox(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(())
    }

    async fn pod_sandbox_status(&self, sandbox_id: &str) -> CriResult<t::PodSandboxStatus> {
        let req = proto::PodSandboxStatusRequest {
            pod_sandbox_id: sandbox_id.to_owned(),
            verbose: false,
        };
        let resp = self
            .runtime
            .clone()
            .pod_sandbox_status(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        resp.into_inner()
            .status
            .map(Into::into)
            .ok_or_else(|| CriError::NotFound(format!("pod sandbox {sandbox_id}")))
    }

    async fn list_pod_sandbox(
        &self,
        filter: Option<t::PodSandboxFilter>,
    ) -> CriResult<Vec<t::PodSandbox>> {
        let req = proto::ListPodSandboxRequest {
            filter: filter.map(Into::into),
        };
        let resp = self
            .runtime
            .clone()
            .list_pod_sandbox(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(resp
            .into_inner()
            .items
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn create_container(
        &self,
        sandbox_id: &str,
        cfg: t::ContainerConfig,
        sandbox_cfg: t::PodSandboxConfig,
    ) -> CriResult<String> {
        let req = proto::CreateContainerRequest {
            pod_sandbox_id: sandbox_id.to_owned(),
            config: Some(cfg.into()),
            sandbox_config: Some(sandbox_cfg.into()),
        };
        let resp = self
            .runtime
            .clone()
            .create_container(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(resp.into_inner().container_id)
    }

    async fn start_container(&self, container_id: &str) -> CriResult<()> {
        let req = proto::StartContainerRequest {
            container_id: container_id.to_owned(),
        };
        self.runtime
            .clone()
            .start_container(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(())
    }

    async fn stop_container(&self, container_id: &str, timeout_seconds: i64) -> CriResult<()> {
        let req = proto::StopContainerRequest {
            container_id: container_id.to_owned(),
            timeout: timeout_seconds,
        };
        self.runtime
            .clone()
            .stop_container(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(())
    }

    async fn remove_container(&self, container_id: &str) -> CriResult<()> {
        let req = proto::RemoveContainerRequest {
            container_id: container_id.to_owned(),
        };
        self.runtime
            .clone()
            .remove_container(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(())
    }

    async fn container_status(&self, container_id: &str) -> CriResult<t::ContainerStatus> {
        let req = proto::ContainerStatusRequest {
            container_id: container_id.to_owned(),
            verbose: false,
        };
        let resp = self
            .runtime
            .clone()
            .container_status(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        resp.into_inner()
            .status
            .map(Into::into)
            .ok_or_else(|| CriError::NotFound(format!("container {container_id}")))
    }

    async fn list_containers(
        &self,
        filter: Option<t::ContainerFilter>,
    ) -> CriResult<Vec<t::Container>> {
        let req = proto::ListContainersRequest {
            filter: filter.map(Into::into),
        };
        let resp = self
            .runtime
            .clone()
            .list_containers(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(resp
            .into_inner()
            .containers
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn pull_image(&self, image: t::ImageSpec) -> CriResult<String> {
        let req = proto::PullImageRequest {
            image: Some(image.into()),
            auth: None,
            sandbox_config: None,
        };
        let resp = self
            .image
            .clone()
            .pull_image(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(resp.into_inner().image_ref)
    }

    async fn image_status(&self, image: t::ImageSpec) -> CriResult<Option<t::Image>> {
        let req = proto::ImageStatusRequest {
            image: Some(image.into()),
            verbose: false,
        };
        let resp = self
            .image
            .clone()
            .image_status(req)
            .await
            .map_err(|s| status_to_cri_error(&s))?;
        Ok(resp.into_inner().image.map(Into::into))
    }
}
