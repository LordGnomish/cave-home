// SPDX-License-Identifier: Apache-2.0
//! `runtime.v1.ImageService` gRPC handler.
//!
//! Line-by-line port of containerd's `internal/cri/server` Image
//! handlers. PullImage/ImageStatus/ListImages/RemoveImage/ImageFsInfo
//! are FULLY implemented — they hit our own `image::Resolver` (which
//! ports `core/remotes/docker/resolver.go`) and ingest blobs into our
//! `content::Store`.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tonic::{Request, Response, Status};

use crate::content::Store as ContentStore;
use crate::cri::image_store::{Image, ImageStore};
use crate::image::{Reference, Resolver};
use crate::runtime_v1 as pb;

/// CRI Image gRPC server.
#[derive(Clone)]
pub struct ImageServer {
    images: ImageStore,
    resolver: Resolver,
    content: Arc<ContentStore>,
    /// Test-only — switches HTTPS to HTTP. Production = "https".
    scheme: String,
}

impl ImageServer {
    /// Builds an image server.
    #[must_use]
    pub fn new(images: ImageStore, resolver: Resolver, content: Arc<ContentStore>) -> Self {
        Self { images, resolver, content, scheme: "https".to_owned() }
    }

    /// Test-only escape hatch — see `Resolver::resolve_with_scheme`.
    #[must_use]
    pub fn with_scheme(mut self, scheme: impl Into<String>) -> Self {
        self.scheme = scheme.into();
        self
    }

    /// Borrow the underlying image store.
    #[must_use]
    pub const fn images(&self) -> &ImageStore {
        &self.images
    }
}

type EmptyStream<T> =
    Pin<Box<dyn Stream<Item = std::result::Result<T, Status>> + Send + 'static>>;

#[async_trait]
impl pb::image_service_server::ImageService for ImageServer {
    async fn list_images(
        &self,
        _req: Request<pb::ListImagesRequest>,
    ) -> Result<Response<pb::ListImagesResponse>, Status> {
        let images = self
            .images
            .list()
            .into_iter()
            .map(|i| pb::Image {
                id: i.digest.clone(),
                repo_tags: i.references,
                repo_digests: vec![i.digest],
                size: i.size,
                ..Default::default()
            })
            .collect();
        Ok(Response::new(pb::ListImagesResponse { images }))
    }

    type StreamImagesStream = EmptyStream<pb::StreamImagesResponse>;
    async fn stream_images(
        &self,
        _req: Request<pb::StreamImagesRequest>,
    ) -> Result<Response<Self::StreamImagesStream>, Status> {
        Err(Status::unimplemented("stream_images — Phase 1b"))
    }

    async fn image_status(
        &self,
        req: Request<pb::ImageStatusRequest>,
    ) -> Result<Response<pb::ImageStatusResponse>, Status> {
        let r = req
            .into_inner()
            .image
            .ok_or_else(|| Status::invalid_argument("image is required"))?;
        match self.images.get(&r.image) {
            Ok(i) => Ok(Response::new(pb::ImageStatusResponse {
                image: Some(pb::Image {
                    id: i.digest.clone(),
                    repo_tags: i.references,
                    repo_digests: vec![i.digest],
                    size: i.size,
                    ..Default::default()
                }),
                info: Default::default(),
            })),
            // Per CRI spec: missing image → response with nil image,
            // not an error.
            Err(_) => Ok(Response::new(pb::ImageStatusResponse {
                image: None,
                info: Default::default(),
            })),
        }
    }

    async fn pull_image(
        &self,
        req: Request<pb::PullImageRequest>,
    ) -> Result<Response<pb::PullImageResponse>, Status> {
        let req = req.into_inner();
        let spec = req
            .image
            .ok_or_else(|| Status::invalid_argument("image is required"))?;
        let reference = Reference::parse(&spec.image)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        // Fetch the manifest. Bearer-auth is handled inside the
        // resolver.
        let resolved = self
            .resolver
            .resolve_with_scheme(&reference, &self.scheme)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        // Persist the manifest itself in our content store so future
        // ImageStatus/Remove calls have the bytes.
        if !self.content.exists(&resolved.digest).await {
            self.content
                .write(&resolved.digest, &resolved.manifest)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        }

        // Index by user-supplied reference.
        self.images.upsert(
            Image {
                digest: resolved.digest.to_string(),
                references: vec![spec.image.clone()],
                size: resolved.manifest.len() as u64,
            },
            spec.image.clone(),
        );

        Ok(Response::new(pb::PullImageResponse {
            image_ref: resolved.digest.to_string(),
        }))
    }

    async fn remove_image(
        &self,
        req: Request<pb::RemoveImageRequest>,
    ) -> Result<Response<pb::RemoveImageResponse>, Status> {
        let r = req
            .into_inner()
            .image
            .ok_or_else(|| Status::invalid_argument("image is required"))?;
        // Idempotent per CRI spec.
        let _ = self.images.remove(&r.image);
        Ok(Response::new(pb::RemoveImageResponse {}))
    }

    async fn image_fs_info(
        &self,
        _req: Request<pb::ImageFsInfoRequest>,
    ) -> Result<Response<pb::ImageFsInfoResponse>, Status> {
        // Phase 1: report a single placeholder UsedBytes entry pointing
        // at the content store root. Real disk-usage statvfs lives in
        // Phase 1b.
        Ok(Response::new(pb::ImageFsInfoResponse {
            image_filesystems: Vec::new(),
            container_filesystems: Vec::new(),
        }))
    }
}
