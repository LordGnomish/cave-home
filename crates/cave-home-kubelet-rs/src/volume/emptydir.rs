// SPDX-License-Identifier: Apache-2.0
//! `EmptyDirPlugin`.
//!
//! Hand-port of `pkg/volume/emptydir/empty_dir.go` (v1.36.1). Phase 1
//! supports the default node-disk medium only; `Memory`/`HugePages` mediums
//! are deferred to Phase 1b (recorded as `[[unmapped]]`).

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::plugin::{VolumePlugin, VolumeResult};
use crate::api::{PodUid, Volume, VolumeSource};

/// Default kubelet pods root: `/var/lib/cave-home-kubelet/pods`.
pub const DEFAULT_PODS_ROOT: &str = "/var/lib/cave-home-kubelet/pods";

/// `EmptyDirPlugin`.
pub struct EmptyDirPlugin {
    pods_root: PathBuf,
}

impl Default for EmptyDirPlugin {
    fn default() -> Self {
        Self::new(Path::new(DEFAULT_PODS_ROOT))
    }
}

impl EmptyDirPlugin {
    pub fn new(pods_root: &Path) -> Self {
        Self {
            pods_root: pods_root.to_path_buf(),
        }
    }

    /// Path on disk for the given (pod_uid, volume_name): mirrors
    /// `kubelet.getPodVolumeDir(uid, "kubernetes.io~empty-dir", name)`.
    pub fn host_path(&self, pod_uid: &PodUid, name: &str) -> PathBuf {
        self.pods_root
            .join(pod_uid.as_str())
            .join("volumes")
            .join("kubernetes.io~empty-dir")
            .join(name)
    }
}

#[async_trait]
impl VolumePlugin for EmptyDirPlugin {
    fn name(&self) -> &'static str {
        "kubernetes.io/empty-dir"
    }

    fn can_support(&self, volume: &Volume) -> bool {
        matches!(volume.source, VolumeSource::EmptyDir(_))
    }

    async fn set_up(&self, pod_uid: &PodUid, volume: &Volume) -> VolumeResult<PathBuf> {
        let path = self.host_path(pod_uid, &volume.name);
        // `create_dir_all` is the upstream behaviour: the volume dir is
        // created on every set_up (idempotent).
        tokio::fs::create_dir_all(&path).await?;
        Ok(path)
    }

    async fn tear_down(&self, pod_uid: &PodUid, volume: &Volume) -> VolumeResult<()> {
        let path = self.host_path(pod_uid, &volume.name);
        match tokio::fs::remove_dir_all(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
