// SPDX-License-Identifier: Apache-2.0
//! Overlayfs snapshotter.
//!
//! Line-by-line port of containerd's
//! `plugins/snapshots/overlay/overlay.go` (v2.3.0). Phase 1 ships the
//! metadata + on-disk layout half (`fs/`, `work/`, parent-key tracking,
//! `Mounts` returning `lowerdir/upperdir/workdir` strings). The actual
//! `mount(2)` syscall is the caller's responsibility — Phase 1b will
//! add a thin mount helper. See `parity.manifest.toml`.
//!
//! The metastore is in-memory (a `parking_lot::Mutex<HashMap>`) rather
//! than the bbolt-backed `storage.MetaStore` upstream uses; for Phase 1
//! we only need the metadata APIs and the on-disk dir layout, so an
//! in-process store is faithful and avoids dragging in bbolt-rs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio::fs;

/// Errors returned by the snapshotter.
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// Snapshot key not found.
    #[error("snapshot {0:?}: not found")]
    NotFound(String),
    /// Key already exists.
    #[error("snapshot {0:?}: already exists")]
    AlreadyExists(String),
    /// Parent key does not exist.
    #[error("snapshot parent {0:?}: not found")]
    ParentNotFound(String),
    /// Wrong-state operation (e.g. Commit on a View).
    #[error("snapshot {0:?}: invalid state for operation")]
    InvalidState(String),
    /// Underlying I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Snapshot kind — mirrors `snapshots.Kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Active read-write snapshot (has `work/`).
    Active,
    /// Read-only view of a parent.
    View,
    /// Committed snapshot (becomes a parent).
    Committed,
}

/// Snapshot metadata.
#[derive(Debug, Clone)]
pub struct Info {
    /// Stable key (the caller's name).
    pub name: String,
    /// Kind: Active / View / Committed.
    pub kind: Kind,
    /// Parent key (chain root iff empty).
    pub parent: Option<String>,
}

/// A mount specification — mirrors `mount.Mount` from upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// `"overlay"` or `"bind"`.
    pub mount_type: String,
    /// `"overlay"` (or upperdir path for bind).
    pub source: String,
    /// `lowerdir=...,upperdir=...,workdir=...` for overlay; `rw,rbind`
    /// etc. for bind.
    pub options: Vec<String>,
}

/// Internal record per snapshot — id + the parent chain of *committed*
/// IDs (closest first), for the `mounts()` lowerdir computation.
#[derive(Debug, Clone)]
struct Record {
    id: String,
    info: Info,
    /// Parent chain of committed IDs, closest first. For an Active
    /// snapshot whose parent is committed `base` (id "1"), this is
    /// `["1"]`. For a View on a chain root with no parents, this is
    /// empty.
    parent_ids: Vec<String>,
}

/// Overlayfs snapshotter — Phase 1 metadata + filesystem layout.
#[derive(Debug)]
pub struct Snapshotter {
    root: PathBuf,
    state: parking_lot::Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    snapshots: HashMap<String, Record>,
    next_id: u64,
}

impl Snapshotter {
    /// Opens (creates) a new overlay snapshotter rooted at `root`.
    ///
    /// Mirrors upstream `NewSnapshotter()` minus the `SupportsDType`
    /// probe (we'll re-introduce it in Phase 1b alongside the real
    /// mount helper).
    pub async fn open(root: impl AsRef<Path>) -> Result<Self, SnapshotError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("snapshots")).await?;
        Ok(Self { root, state: parking_lot::Mutex::new(State::default()) })
    }

    fn upper_path(&self, id: &str) -> PathBuf {
        self.root.join("snapshots").join(id).join("fs")
    }

    fn work_path(&self, id: &str) -> PathBuf {
        self.root.join("snapshots").join(id).join("work")
    }

    /// Mints the next snapshot ID and reserves the on-disk dir.
    async fn allocate(&self, kind: Kind) -> Result<String, SnapshotError> {
        let id = {
            let mut st = self.state.lock();
            st.next_id += 1;
            st.next_id.to_string()
        };
        let dir = self.root.join("snapshots").join(&id);
        fs::create_dir_all(dir.join("fs")).await?;
        if matches!(kind, Kind::Active) {
            fs::create_dir_all(dir.join("work")).await?;
        }
        Ok(id)
    }

    /// Prepares a new active snapshot keyed by `key`, optionally
    /// chained from `parent`.
    pub async fn prepare(
        &self,
        key: &str,
        parent: Option<&str>,
    ) -> Result<Vec<Mount>, SnapshotError> {
        self.create(Kind::Active, key, parent).await
    }

    /// Prepares a read-only view.
    pub async fn view(
        &self,
        key: &str,
        parent: Option<&str>,
    ) -> Result<Vec<Mount>, SnapshotError> {
        self.create(Kind::View, key, parent).await
    }

    async fn create(
        &self,
        kind: Kind,
        key: &str,
        parent: Option<&str>,
    ) -> Result<Vec<Mount>, SnapshotError> {
        // Resolve the parent chain BEFORE we mutate state (so we don't
        // half-create on error).
        let parent_ids: Vec<String> = if let Some(p) = parent {
            let st = self.state.lock();
            let rec = st
                .snapshots
                .get(p)
                .ok_or_else(|| SnapshotError::ParentNotFound(p.to_owned()))?;
            if rec.info.kind != Kind::Committed {
                return Err(SnapshotError::InvalidState(p.to_owned()));
            }
            // chain = [parent's id, ...parent's own parent_ids]
            let mut v = Vec::with_capacity(rec.parent_ids.len() + 1);
            v.push(rec.id.clone());
            v.extend(rec.parent_ids.iter().cloned());
            v
        } else {
            Vec::new()
        };

        // Pre-check duplicate key without holding the lock across an
        // await.
        {
            let st = self.state.lock();
            if st.snapshots.contains_key(key) {
                return Err(SnapshotError::AlreadyExists(key.to_owned()));
            }
        }

        let id = self.allocate(kind).await?;
        let info = Info {
            name: key.to_owned(),
            kind,
            parent: parent.map(str::to_owned),
        };

        // Final installation under the lock.
        {
            let mut st = self.state.lock();
            // race: another caller could have inserted while we awaited.
            if st.snapshots.contains_key(key) {
                return Err(SnapshotError::AlreadyExists(key.to_owned()));
            }
            st.snapshots.insert(
                key.to_owned(),
                Record { id: id.clone(), info: info.clone(), parent_ids: parent_ids.clone() },
            );
        }

        Ok(self.mounts_for(kind, &id, &parent_ids))
    }

    /// Returns the mounts for an existing snapshot.
    pub async fn mounts(&self, key: &str) -> Result<Vec<Mount>, SnapshotError> {
        let st = self.state.lock();
        let rec = st
            .snapshots
            .get(key)
            .ok_or_else(|| SnapshotError::NotFound(key.to_owned()))?;
        Ok(self.mounts_for(rec.info.kind, &rec.id, &rec.parent_ids))
    }

    /// Mirrors upstream `(*snapshotter).mounts(s, info)` —
    /// overlay.go:552-615.
    fn mounts_for(&self, kind: Kind, id: &str, parent_ids: &[String]) -> Vec<Mount> {
        // No parents → bind mount on this snapshot's own upper dir.
        if parent_ids.is_empty() {
            let ro = matches!(kind, Kind::View);
            return vec![Mount {
                mount_type: "bind".to_owned(),
                source: self.upper_path(id).to_string_lossy().into_owned(),
                options: vec![if ro { "ro" } else { "rw" }.to_owned(), "rbind".to_owned()],
            }];
        }

        // Active over a parent → overlay with workdir+upperdir.
        let mut options: Vec<String> = Vec::new();
        if matches!(kind, Kind::Active) {
            options.push(format!("workdir={}", self.work_path(id).display()));
            options.push(format!("upperdir={}", self.upper_path(id).display()));
        } else if parent_ids.len() == 1 {
            // Single-parent View → ro,rbind on parent's upper.
            return vec![Mount {
                mount_type: "bind".to_owned(),
                source: self.upper_path(&parent_ids[0]).to_string_lossy().into_owned(),
                options: vec!["ro".to_owned(), "rbind".to_owned()],
            }];
        }

        let lowers: Vec<String> = parent_ids
            .iter()
            .map(|pid| self.upper_path(pid).to_string_lossy().into_owned())
            .collect();
        options.push(format!("lowerdir={}", lowers.join(":")));

        vec![Mount {
            mount_type: "overlay".to_owned(),
            source: "overlay".to_owned(),
            options,
        }]
    }

    /// Commits the active snapshot at `key` under the new committed
    /// `name`; the active key is consumed.
    pub async fn commit(&self, name: &str, key: &str) -> Result<(), SnapshotError> {
        let mut st = self.state.lock();
        if st.snapshots.contains_key(name) {
            return Err(SnapshotError::AlreadyExists(name.to_owned()));
        }
        let mut rec = st
            .snapshots
            .remove(key)
            .ok_or_else(|| SnapshotError::NotFound(key.to_owned()))?;
        if rec.info.kind != Kind::Active {
            // Re-insert so we don't lose the active record on caller
            // error.
            st.snapshots.insert(key.to_owned(), rec);
            return Err(SnapshotError::InvalidState(key.to_owned()));
        }
        rec.info = Info {
            name: name.to_owned(),
            kind: Kind::Committed,
            parent: rec.info.parent.clone(),
        };
        st.snapshots.insert(name.to_owned(), rec);
        Ok(())
    }

    /// Returns metadata for the snapshot.
    pub async fn stat(&self, key: &str) -> Result<Info, SnapshotError> {
        let st = self.state.lock();
        st.snapshots
            .get(key)
            .map(|r| r.info.clone())
            .ok_or_else(|| SnapshotError::NotFound(key.to_owned()))
    }

    /// Removes the snapshot identified by `key`.
    pub async fn remove(&self, key: &str) -> Result<(), SnapshotError> {
        let id = {
            let mut st = self.state.lock();
            let rec = st
                .snapshots
                .remove(key)
                .ok_or_else(|| SnapshotError::NotFound(key.to_owned()))?;
            rec.id
        };
        let dir = self.root.join("snapshots").join(&id);
        if let Err(e) = fs::remove_dir_all(&dir).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(SnapshotError::Io(e));
            }
        }
        Ok(())
    }

    /// Walks all snapshots, calling `visitor` for each.
    pub async fn walk<F>(&self, mut visitor: F) -> Result<(), SnapshotError>
    where
        F: FnMut(&Info),
    {
        let st = self.state.lock();
        for rec in st.snapshots.values() {
            visitor(&rec.info);
        }
        Ok(())
    }

    /// Updates metadata (Phase 1: name + parent are immutable; kind is
    /// not user-mutable; this is effectively an identity op).
    pub async fn update(&self, info: Info) -> Result<Info, SnapshotError> {
        let st = self.state.lock();
        let rec = st
            .snapshots
            .get(&info.name)
            .ok_or_else(|| SnapshotError::NotFound(info.name.clone()))?;
        Ok(rec.info.clone())
    }
}
