// SPDX-License-Identifier: Apache-2.0
//! `HostPathPlugin`.
//!
//! Hand-port of `pkg/volume/hostpath/host_path.go::checkType` (v1.36.1).
//!
//! Phase 1 supports the following `HostPathType` values:
//!   - `Unset`   — no validation
//!   - `Directory`, `DirectoryOrCreate`
//!   - `File`,      `FileOrCreate`
//!   - `Socket`
//!
//! `CharDevice` and `BlockDevice` are recorded as `[[unmapped]]` (Phase 1b)
//! because they need a Linux-only metadata syscall (`stat::S_ISCHR/S_ISBLK`).

use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;

use async_trait::async_trait;

use super::plugin::{VolumeError, VolumePlugin, VolumeResult};
use crate::api::{HostPathType, PodUid, Volume, VolumeSource};

#[derive(Default)]
pub struct HostPathPlugin;

impl HostPathPlugin {
    pub fn new() -> Self {
        Self
    }
}

fn check_type(path: &std::path::Path, t: HostPathType) -> VolumeResult<()> {
    let meta_res = std::fs::symlink_metadata(path);
    match (t, meta_res) {
        (HostPathType::Unset, _) => Ok(()),
        (HostPathType::DirectoryOrCreate, Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(path)?;
            Ok(())
        }
        (HostPathType::DirectoryOrCreate, Err(e)) => Err(VolumeError::InvalidHostPath(format!(
            "{}: {e}",
            path.display()
        ))),
        (HostPathType::DirectoryOrCreate, Ok(m)) if m.is_dir() => Ok(()),
        (HostPathType::DirectoryOrCreate, Ok(_)) => Err(VolumeError::InvalidHostPath(format!(
            "{} exists but is not a directory",
            path.display()
        ))),
        (HostPathType::Directory, Ok(m)) if m.is_dir() => Ok(()),
        (HostPathType::Directory, Ok(_)) => Err(VolumeError::InvalidHostPath(format!(
            "{} is not a directory",
            path.display()
        ))),
        (HostPathType::Directory, Err(e)) => Err(VolumeError::InvalidHostPath(format!(
            "{}: {e}",
            path.display()
        ))),
        (HostPathType::FileOrCreate, Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            // Touch the file.
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(path)?;
            Ok(())
        }
        (HostPathType::FileOrCreate, Err(e)) => Err(VolumeError::InvalidHostPath(format!(
            "{}: {e}",
            path.display()
        ))),
        (HostPathType::FileOrCreate, Ok(m)) if m.is_file() => Ok(()),
        (HostPathType::FileOrCreate, Ok(_)) => Err(VolumeError::InvalidHostPath(format!(
            "{} exists but is not a regular file",
            path.display()
        ))),
        (HostPathType::File, Ok(m)) if m.is_file() => Ok(()),
        (HostPathType::File, Ok(_)) => Err(VolumeError::InvalidHostPath(format!(
            "{} is not a regular file",
            path.display()
        ))),
        (HostPathType::File, Err(e)) => Err(VolumeError::InvalidHostPath(format!(
            "{}: {e}",
            path.display()
        ))),
        (HostPathType::Socket, Ok(m)) if m.file_type().is_socket() => Ok(()),
        (HostPathType::Socket, Ok(_)) => Err(VolumeError::InvalidHostPath(format!(
            "{} is not a unix socket",
            path.display()
        ))),
        (HostPathType::Socket, Err(e)) => Err(VolumeError::InvalidHostPath(format!(
            "{}: {e}",
            path.display()
        ))),
        // Char/Block devices need stat::S_ISCHR/S_ISBLK — Phase 1b.
        (HostPathType::CharDevice | HostPathType::BlockDevice, _) => Err(VolumeError::Unsupported(
            "HostPathType::CharDevice/BlockDevice (Phase 1b)",
        )),
    }
}

#[async_trait]
impl VolumePlugin for HostPathPlugin {
    fn name(&self) -> &'static str {
        "kubernetes.io/host-path"
    }

    fn can_support(&self, volume: &Volume) -> bool {
        matches!(volume.source, VolumeSource::HostPath(_))
    }

    async fn set_up(&self, _pod_uid: &PodUid, volume: &Volume) -> VolumeResult<PathBuf> {
        let VolumeSource::HostPath(hp) = &volume.source else {
            return Err(VolumeError::Unsupported(
                "HostPathPlugin::set_up requires HostPath",
            ));
        };
        let path = PathBuf::from(&hp.path);
        check_type(&path, hp.host_path_type)?;
        Ok(path)
    }

    async fn tear_down(&self, _pod_uid: &PodUid, _volume: &Volume) -> VolumeResult<()> {
        // hostPath is owned by the host: tear-down is intentionally a no-op,
        // matching upstream behaviour in `pkg/volume/hostpath/host_path.go`.
        Ok(())
    }
}
