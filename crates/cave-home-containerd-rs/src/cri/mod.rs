// SPDX-License-Identifier: Apache-2.0
//! CRI v1 gRPC server — line-by-line port of containerd's
//! `internal/cri/server` Go package, scoped to Phase 1 metadata-level
//! parity. See `parity.manifest.toml` for the mapped RPC table.

pub mod container_store;
pub mod errors;
pub mod image_service;
pub mod image_store;
pub mod runtime_service;
pub mod sandbox_store;
pub mod types;

pub use container_store::{Container, ContainerStore};
pub use errors::CriError;
pub use image_service::ImageServer;
pub use image_store::{Image, ImageStore};
pub use runtime_service::RuntimeServer;
pub use sandbox_store::{Sandbox, SandboxStore};
pub use types::{
    ContainerMetadata, ContainerState, ContainerStatus, SandboxMetadata, SandboxState,
    SandboxStatus,
};
