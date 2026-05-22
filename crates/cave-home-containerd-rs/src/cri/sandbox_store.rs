// SPDX-License-Identifier: Apache-2.0
//! In-memory PodSandbox store.
//!
//! Line-by-line port of containerd's
//! `internal/cri/store/sandbox/sandbox.go` (v2.3.0). The truncindex
//! / label store / netns side-channels are deferred to Phase 1b — see
//! `parity.manifest.toml`.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::cri::errors::CriError;
use crate::cri::types::{SandboxMetadata, SandboxStatus};

/// A single sandbox entry — metadata + mutable status.
#[derive(Debug, Clone)]
pub struct Sandbox {
    /// Immutable metadata.
    pub metadata: SandboxMetadata,
    /// Mutable status.
    pub status: SandboxStatus,
}

/// Thread-safe map of `sandbox_id → Sandbox`.
#[derive(Debug, Default, Clone)]
pub struct SandboxStore {
    inner: Arc<RwLock<HashMap<String, Sandbox>>>,
}

impl SandboxStore {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a sandbox; errors if the ID already exists.
    /// Mirrors `(*Store).Add` (sandbox.go:95).
    pub fn add(&self, sb: Sandbox) -> Result<(), CriError> {
        let mut g = self.inner.write();
        if g.contains_key(&sb.metadata.id) {
            return Err(CriError::AlreadyExists(format!("sandbox {}", sb.metadata.id)));
        }
        g.insert(sb.metadata.id.clone(), sb);
        Ok(())
    }

    /// Looks up a sandbox by ID. Mirrors `(*Store).Get` (sandbox.go:116).
    pub fn get(&self, id: &str) -> Result<Sandbox, CriError> {
        self.inner
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| CriError::NotFound(format!("sandbox {id}")))
    }

    /// Returns all sandboxes. Order is unspecified (matches upstream).
    /// Mirrors `(*Store).List` (sandbox.go:133).
    #[must_use]
    pub fn list(&self) -> Vec<Sandbox> {
        self.inner.read().values().cloned().collect()
    }

    /// Mutates the status of an existing sandbox. Errors if absent.
    pub fn update_status<F>(&self, id: &str, mutator: F) -> Result<(), CriError>
    where
        F: FnOnce(&mut SandboxStatus),
    {
        let mut g = self.inner.write();
        let sb = g
            .get_mut(id)
            .ok_or_else(|| CriError::NotFound(format!("sandbox {id}")))?;
        mutator(&mut sb.status);
        Ok(())
    }

    /// Removes a sandbox. No error on missing-ID — matches upstream
    /// `(*Store).Delete` (sandbox.go:168), which silently returns.
    pub fn delete(&self, id: &str) {
        self.inner.write().remove(id);
    }
}
