// SPDX-License-Identifier: Apache-2.0
//! In-memory Container store.
//!
//! Line-by-line port of containerd's
//! `internal/cri/store/container/container.go` (v2.3.0). IO handles,
//! containerd-client handles, and the stats collector are deferred to
//! Phase 1b — see `parity.manifest.toml`.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::cri::errors::CriError;
use crate::cri::types::{ContainerMetadata, ContainerState, ContainerStatus};

/// A single container entry — metadata + mutable status.
#[derive(Debug, Clone)]
pub struct Container {
    /// Immutable metadata.
    pub metadata: ContainerMetadata,
    /// Mutable status.
    pub status: ContainerStatus,
}

/// Thread-safe map of `container_id → Container`.
#[derive(Debug, Default, Clone)]
pub struct ContainerStore {
    inner: Arc<RwLock<HashMap<String, Container>>>,
}

impl ContainerStore {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mirrors `(*Store).Add` (container.go:129).
    pub fn add(&self, c: Container) -> Result<(), CriError> {
        let mut g = self.inner.write();
        if g.contains_key(&c.metadata.id) {
            return Err(CriError::AlreadyExists(format!("container {}", c.metadata.id)));
        }
        g.insert(c.metadata.id.clone(), c);
        Ok(())
    }

    /// Mirrors `(*Store).Get` (container.go:150).
    pub fn get(&self, id: &str) -> Result<Container, CriError> {
        self.inner
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| CriError::NotFound(format!("container {id}")))
    }

    /// Mirrors `(*Store).List` (container.go:167).
    #[must_use]
    pub fn list(&self) -> Vec<Container> {
        self.inner.read().values().cloned().collect()
    }

    /// Returns all containers belonging to `sandbox_id`.
    #[must_use]
    pub fn list_for_sandbox(&self, sandbox_id: &str) -> Vec<Container> {
        self.inner
            .read()
            .values()
            .filter(|c| c.metadata.sandbox_id == sandbox_id)
            .cloned()
            .collect()
    }

    /// Mutates the status of an existing container.
    pub fn update_status<F>(&self, id: &str, mutator: F) -> Result<(), CriError>
    where
        F: FnOnce(&mut ContainerStatus),
    {
        let mut g = self.inner.write();
        let c = g
            .get_mut(id)
            .ok_or_else(|| CriError::NotFound(format!("container {id}")))?;
        mutator(&mut c.status);
        Ok(())
    }

    /// Refuses to remove a Running container (matches upstream
    /// `RemoveContainer` semantics — kubelet must Stop first).
    pub fn delete(&self, id: &str) -> Result<(), CriError> {
        let mut g = self.inner.write();
        let c = g
            .get(id)
            .ok_or_else(|| CriError::NotFound(format!("container {id}")))?;
        if c.status.state == ContainerState::Running {
            return Err(CriError::FailedPrecondition(format!(
                "container {id} is still running"
            )));
        }
        g.remove(id);
        Ok(())
    }
}
