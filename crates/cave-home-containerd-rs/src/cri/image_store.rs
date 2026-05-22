// SPDX-License-Identifier: Apache-2.0
//! In-memory Image store — matches a subset of
//! `internal/cri/store/image/image.go`. Phase 1 only needs ref → digest
//! tracking + size for `ListImages` / `ImageStatus`.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::cri::errors::CriError;

/// One image entry.
#[derive(Debug, Clone)]
pub struct Image {
    /// Manifest digest (`sha256:…`).
    pub digest: String,
    /// User-supplied references (`registry/repo:tag`).
    pub references: Vec<String>,
    /// On-disk size in bytes (from manifest).
    pub size: u64,
}

/// Thread-safe `digest → Image` plus `ref → digest` index.
#[derive(Debug, Default, Clone)]
pub struct ImageStore {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    by_digest: HashMap<String, Image>,
    /// `reference → digest`
    by_ref: HashMap<String, String>,
}

impl ImageStore {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts (or upserts) an image. Adds `r#ref → digest` index.
    pub fn upsert(&self, image: Image, reference: String) {
        let mut g = self.inner.write();
        let dgst = image.digest.clone();
        g.by_ref.insert(reference.clone(), dgst.clone());
        let entry = g.by_digest.entry(dgst).or_insert(image);
        if !entry.references.contains(&reference) {
            entry.references.push(reference);
        }
    }

    /// Looks up by either tag or digest.
    pub fn get(&self, reference: &str) -> Result<Image, CriError> {
        let g = self.inner.read();
        let dgst = if reference.starts_with("sha256:") {
            reference.to_owned()
        } else {
            g.by_ref
                .get(reference)
                .cloned()
                .ok_or_else(|| CriError::NotFound(format!("image {reference}")))?
        };
        g.by_digest
            .get(&dgst)
            .cloned()
            .ok_or_else(|| CriError::NotFound(format!("image {reference}")))
    }

    /// Lists all known images.
    #[must_use]
    pub fn list(&self) -> Vec<Image> {
        self.inner.read().by_digest.values().cloned().collect()
    }

    /// Removes an image by reference. Returns NotFound if absent.
    pub fn remove(&self, reference: &str) -> Result<(), CriError> {
        let mut g = self.inner.write();
        let dgst = if reference.starts_with("sha256:") {
            reference.to_owned()
        } else {
            g.by_ref
                .remove(reference)
                .ok_or_else(|| CriError::NotFound(format!("image {reference}")))?
        };
        if g.by_digest.remove(&dgst).is_none() {
            return Err(CriError::NotFound(format!("image {reference}")));
        }
        // Also drop any other references that point at this digest.
        g.by_ref.retain(|_, v| v != &dgst);
        Ok(())
    }
}
