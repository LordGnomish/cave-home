// SPDX-License-Identifier: Apache-2.0
//! Tonic gRPC server bootstrap. Lib-side only — the binary entry point
//! lives in `cave-home-binary` per the unified-binary charter (§5).
//!
//! `serve()` returns a `tonic::transport::server::Router` ready for the
//! caller to `.serve(addr).await` on. Wiring the addr (Unix socket vs
//! TCP) is deferred to the binary so test fixtures can use channels
//! instead.

use std::sync::Arc;

use crate::content::Store as ContentStore;
use crate::cri::{
    ContainerStore, ImageServer, ImageStore, RuntimeServer, SandboxStore,
};
use crate::image::Resolver;
use crate::runtime_v1::image_service_server::ImageServiceServer;
use crate::runtime_v1::runtime_service_server::RuntimeServiceServer;

/// All in-process state for the CRI server.
#[derive(Clone)]
pub struct Cri {
    /// Backed sandbox store.
    pub sandboxes: SandboxStore,
    /// Backed container store.
    pub containers: ContainerStore,
    /// Backed image store.
    pub images: ImageStore,
    /// Content-addressable blob store (filesystem-backed).
    pub content: Arc<ContentStore>,
    /// Image resolver (HTTP).
    pub resolver: Resolver,
}

impl Cri {
    /// Builds a fresh CRI state container with empty stores.
    #[must_use]
    pub fn new(content: Arc<ContentStore>, http: reqwest::Client) -> Self {
        let resolver = Resolver::new(http, content.clone());
        Self {
            sandboxes: SandboxStore::new(),
            containers: ContainerStore::new(),
            images: ImageStore::new(),
            content,
            resolver,
        }
    }
}

/// Wraps the runtime + image gRPC handlers into a configured tonic
/// `Router`. The binary is responsible for picking the listener
/// (TCP / Unix socket) and calling `.serve()`.
#[must_use]
pub fn router(cri: Cri) -> tonic::transport::server::Router {
    let runtime = RuntimeServer::new(cri.sandboxes.clone(), cri.containers.clone());
    let images = ImageServer::new(cri.images.clone(), cri.resolver.clone(), cri.content.clone());
    tonic::transport::Server::builder()
        .add_service(RuntimeServiceServer::new(runtime))
        .add_service(ImageServiceServer::new(images))
}
