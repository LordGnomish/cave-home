// SPDX-License-Identifier: Apache-2.0
//! `CriClient` async trait.

use async_trait::async_trait;
use thiserror::Error;

use super::types::{
    Container, ContainerConfig, ContainerFilter, ContainerStatus, FilesystemUsage, Image, ImageSpec,
    PodSandbox, PodSandboxConfig, PodSandboxFilter, PodSandboxStatus,
};

/// Error returned by every `CriClient` method.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CriError {
    /// The requested object does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// The requested operation is invalid in the current state.
    #[error("invalid state: {0}")]
    InvalidState(String),
    /// The runtime answered with a gRPC status other than `NOT_FOUND`.
    ///
    /// `code` is the numeric `tonic::Code` so callers can branch/retry without
    /// the decision core having to depend on the gRPC stack. Only produced by
    /// the `remote-cri` transport.
    #[error("cri rpc failed (code {code}): {message}")]
    Rpc {
        /// Numeric gRPC status code.
        code: i32,
        /// Human-readable status message returned by the runtime.
        message: String,
    },
    /// The gRPC channel/connection itself failed (dial, TLS, broken pipe…).
    /// Only produced by the `remote-cri` transport.
    #[error("cri transport error: {0}")]
    Transport(String),
}

/// Result alias.
pub type CriResult<T> = Result<T, CriError>;

/// Container Runtime Interface client (kubelet -> runtime, e.g. containerd).
///
/// Hand-port of the Go `RuntimeService` + `ImageService` interfaces in
/// `kubelet/cri/remote/`. The set below is the slice the kubelet actually
/// drives in Phase 1.
#[async_trait]
pub trait CriClient: Send + Sync {
    // ---------- runtime version ------------------------------------------------
    async fn version(&self) -> CriResult<String>;

    // ---------- pod sandbox lifecycle ------------------------------------------
    async fn run_pod_sandbox(&self, cfg: PodSandboxConfig) -> CriResult<String>;
    async fn stop_pod_sandbox(&self, sandbox_id: &str) -> CriResult<()>;
    async fn remove_pod_sandbox(&self, sandbox_id: &str) -> CriResult<()>;
    async fn pod_sandbox_status(&self, sandbox_id: &str) -> CriResult<PodSandboxStatus>;
    async fn list_pod_sandbox(
        &self,
        filter: Option<PodSandboxFilter>,
    ) -> CriResult<Vec<PodSandbox>>;

    // ---------- container lifecycle --------------------------------------------
    async fn create_container(
        &self,
        sandbox_id: &str,
        cfg: ContainerConfig,
        sandbox_cfg: PodSandboxConfig,
    ) -> CriResult<String>;
    async fn start_container(&self, container_id: &str) -> CriResult<()>;
    async fn stop_container(&self, container_id: &str, timeout_seconds: i64) -> CriResult<()>;
    async fn remove_container(&self, container_id: &str) -> CriResult<()>;
    async fn container_status(&self, container_id: &str) -> CriResult<ContainerStatus>;
    async fn list_containers(&self, filter: Option<ContainerFilter>) -> CriResult<Vec<Container>>;

    // ---------- image service --------------------------------------------------
    async fn pull_image(&self, image: ImageSpec) -> CriResult<String>;
    async fn image_status(&self, image: ImageSpec) -> CriResult<Option<Image>>;
    /// List images known to the runtime, optionally filtered to a single spec.
    async fn list_images(&self, filter: Option<ImageSpec>) -> CriResult<Vec<Image>>;
    /// Remove an image. Idempotent: removing an absent image succeeds (the CRI
    /// runtime treats "already gone" as success).
    async fn remove_image(&self, image: ImageSpec) -> CriResult<()>;
    /// Report per-filesystem image-store usage (drives image GC).
    async fn image_fs_info(&self) -> CriResult<Vec<FilesystemUsage>>;
}
