// SPDX-License-Identifier: Apache-2.0
//! Local content-addressable blob store.
//!
//! Line-by-line port of containerd's
//! `plugins/content/local/store.go` (v2.3.0). Phase 1 scope:
//!
//!   * sha256-only digests (containerd's `digest.Canonical` is sha256).
//!   * On-disk layout `<root>/blobs/sha256/<hex>` matching upstream's
//!     `blobPath()` exactly.
//!   * Ingest workflow: write → verify → atomic rename.
//!   * `Walk` over committed blobs.
//!
//! Out of scope — see `parity.manifest.toml`:
//!   * fsverity integrity bits
//!   * resumable ingest (`resumeStatus`)
//!   * label store (we use a stub `LabelStore` that returns empty maps)

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest as _, Sha256};
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt as _;

/// Errors returned by [`Store`].
///
/// These mirror the `errdefs` taxonomy used throughout containerd:
///
///   * [`StoreError::NotFound`] ↔ `errdefs.ErrNotFound`
///   * [`StoreError::AlreadyExists`] ↔ `errdefs.ErrAlreadyExists`
///   * [`StoreError::DigestMismatch`] ↔ the sha256-verification failure
///     emitted by upstream's commit step.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The requested blob does not exist in the store.
    #[error("content {0}: not found")]
    NotFound(Digest),
    /// A blob with the same digest is already present (writer collision).
    #[error("content {0}: already exists")]
    AlreadyExists(Digest),
    /// The bytes written do not hash to the expected digest.
    #[error("content {expected}: digest mismatch (got {actual})")]
    DigestMismatch {
        /// The digest the caller asserted.
        expected: Digest,
        /// The digest the store computed from the bytes.
        actual: Digest,
    },
    /// Underlying I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The supplied digest string is malformed (non-`sha256:<hex>`).
    #[error("invalid digest: {0}")]
    InvalidDigest(String),
}

/// A content digest. Phase 1 supports sha256 only — the
/// `digest.Canonical` constant in upstream Go is also sha256.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Digest {
    /// Lower-case hex of the sha256 sum (64 chars).
    hex: String,
}

impl Digest {
    /// Computes the canonical sha256 digest of `bytes`.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut h = Sha256::new();
        h.update(bytes);
        Self { hex: hex::encode(h.finalize()) }
    }

    /// Parses a `sha256:<hex>` string. Lower-cases the hex part.
    pub fn parse(s: &str) -> Result<Self, StoreError> {
        let rest = s
            .strip_prefix("sha256:")
            .ok_or_else(|| StoreError::InvalidDigest(s.to_owned()))?;
        if rest.len() != 64 || !rest.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(StoreError::InvalidDigest(s.to_owned()));
        }
        Ok(Self { hex: rest.to_ascii_lowercase() })
    }

    /// Hex part (without the `sha256:` prefix).
    #[must_use]
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sha256:{}", self.hex)
    }
}

/// Metadata returned by [`Store::info`].
///
/// Mirrors `core/content/content.go`'s `Info` struct, minus the
/// `Labels` map (Phase 1b).
#[derive(Debug, Clone)]
pub struct Info {
    /// The blob's digest.
    pub digest: Digest,
    /// Size in bytes on disk.
    pub size: u64,
    /// Filesystem mtime at commit.
    pub created_at: SystemTime,
}

/// On-disk content-addressable blob store. Cheap to clone — all state
/// is on the filesystem.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Opens (and lazily creates) a content store rooted at `root`.
    ///
    /// Equivalent to upstream's `NewStore(root) (content.Store, error)`
    /// modulo the fsverity probe (Phase 1b).
    pub async fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("blobs/sha256")).await?;
        fs::create_dir_all(root.join("ingest")).await?;
        Ok(Self { root })
    }

    fn blob_path(&self, dgst: &Digest) -> PathBuf {
        self.root.join("blobs/sha256").join(&dgst.hex)
    }

    /// Returns true iff a blob with `dgst` is committed in the store.
    pub async fn exists(&self, dgst: &Digest) -> bool {
        fs::try_exists(self.blob_path(dgst)).await.unwrap_or(false)
    }

    /// Returns metadata for the blob — `errdefs.ErrNotFound` equivalent
    /// when missing.
    pub async fn info(&self, dgst: &Digest) -> Result<Info, StoreError> {
        let p = self.blob_path(dgst);
        let md = match fs::metadata(&p).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::NotFound(dgst.clone()));
            }
            Err(e) => return Err(StoreError::Io(e)),
        };
        let created_at = md.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        Ok(Info { digest: dgst.clone(), size: md.len(), created_at })
    }

    /// Reads the blob bytes back out.
    pub async fn read(&self, dgst: &Digest) -> Result<Vec<u8>, StoreError> {
        let p = self.blob_path(dgst);
        match fs::read(&p).await {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StoreError::NotFound(dgst.clone()))
            }
            Err(e) => Err(StoreError::Io(e)),
        }
    }

    /// Atomically ingests `bytes` and verifies against `expected`.
    ///
    /// This is the single-shot equivalent of upstream's
    /// `Writer().Write().Commit()` chain. We omit the resumable-ingest
    /// path since Phase 1 callers always know the full byte slice up
    /// front (manifest fetch + blob fetch both produce a `Vec<u8>`).
    ///
    /// On digest mismatch, the temp file is removed.
    pub async fn write(&self, expected: &Digest, bytes: &[u8]) -> Result<(), StoreError> {
        // Pre-existing → ErrAlreadyExists, mirroring upstream `writer()`.
        if self.exists(expected).await {
            return Err(StoreError::AlreadyExists(expected.clone()));
        }

        let actual = Digest::from_bytes(bytes);
        if actual != *expected {
            return Err(StoreError::DigestMismatch { expected: expected.clone(), actual });
        }

        // Write to ingest temp, then atomic rename → blob path.
        let ingest = self.root.join("ingest").join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut f = fs::File::create(&ingest).await?;
        f.write_all(bytes).await?;
        f.sync_all().await?;
        drop(f);

        let target = self.blob_path(expected);
        if let Err(e) = fs::rename(&ingest, &target).await {
            // Best-effort cleanup; preserve the original error.
            let _ = fs::remove_file(&ingest).await;
            return Err(StoreError::Io(e));
        }
        Ok(())
    }

    /// Removes a blob. Errors with NotFound if absent.
    pub async fn delete(&self, dgst: &Digest) -> Result<(), StoreError> {
        match fs::remove_file(self.blob_path(dgst)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StoreError::NotFound(dgst.clone()))
            }
            Err(e) => Err(StoreError::Io(e)),
        }
    }

    /// Walks all committed blobs, calling `visitor` for each.
    pub async fn walk<F>(&self, mut visitor: F) -> Result<(), StoreError>
    where
        F: FnMut(&Info),
    {
        let blobs = self.root.join("blobs/sha256");
        let mut rd = match fs::read_dir(&blobs).await {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(StoreError::Io(e)),
        };
        while let Some(entry) = rd.next_entry().await? {
            let name = entry.file_name();
            let Some(hex) = name.to_str() else { continue };
            // Skip anything that isn't a 64-char lower-hex sha256 file.
            if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let dgst = Digest { hex: hex.to_owned() };
            let md = entry.metadata().await?;
            let created_at = md.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            visitor(&Info { digest: dgst, size: md.len(), created_at });
        }
        Ok(())
    }
}
