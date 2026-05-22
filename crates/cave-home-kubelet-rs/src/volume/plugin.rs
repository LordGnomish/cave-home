// SPDX-License-Identifier: Apache-2.0
//! `VolumePlugin` trait + error types.
//!
//! Hand-port of `pkg/volume/plugins.go::VolumePlugin`.

use std::path::PathBuf;

use thiserror::Error;

use crate::api::{PodUid, Volume};

/// Volume manager error.
#[derive(Debug, Error)]
pub enum VolumeError {
    /// Plugin can't handle this volume source kind.
    #[error("unsupported volume source for plugin {0}")]
    Unsupported(&'static str),
    /// Path validation failed.
    #[error("invalid host path: {0}")]
    InvalidHostPath(String),
    /// I/O error during setup/teardown.
    #[error("I/O error: {0}")]
    Io(String),
}

impl From<std::io::Error> for VolumeError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

pub type VolumeResult<T> = Result<T, VolumeError>;

/// Pluggable volume backend.
///
/// Phase 1 implements two: `EmptyDirPlugin`, `HostPathPlugin`.
#[async_trait::async_trait]
pub trait VolumePlugin: Send + Sync {
    /// Stable plugin name (e.g. "kubernetes.io/empty-dir").
    fn name(&self) -> &'static str;

    /// True iff this plugin can handle `volume`.
    fn can_support(&self, volume: &Volume) -> bool;

    /// Set up the volume on disk and return the host path that should be
    /// bind-mounted into the container.
    async fn set_up(&self, pod_uid: &PodUid, volume: &Volume) -> VolumeResult<PathBuf>;

    /// Tear down the volume (remove on-disk state).
    async fn tear_down(&self, pod_uid: &PodUid, volume: &Volume) -> VolumeResult<()>;
}
