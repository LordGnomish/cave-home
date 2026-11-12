// SPDX-License-Identifier: Apache-2.0
//! Unused-image garbage-collection selection.
//!
//! Behavioural reimplementation of the documented containerd/CRI image GC
//! policy: an image is eligible for removal when it is not referenced by any
//! live container. This module computes the eligible set; the actual blob
//! deletion / snapshot teardown is the deferred content/snapshotter layer.
//!
//! Spec source: containerd CRI image GC — images unreferenced by any
//! container (and not pinned) are reclaimable; pinned images are retained.

use std::collections::HashSet;
use std::hash::BuildHasher;

use crate::digest::Digest;

/// A locally-present image record, keyed by its content digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRecord {
    /// The image's content digest (manifest digest).
    pub digest: Digest,
    /// Whether the image is pinned and must never be GC'd.
    pub pinned: bool,
    /// On-disk size in bytes (informational; reported for reclaim accounting).
    pub size: u64,
}

/// Selects images eligible for garbage collection.
///
/// An image is eligible iff it is **not pinned** and **not referenced** by any
/// digest in `in_use`. The result preserves the input order so callers get a
/// deterministic reclaim list, and is returned by reference into `images`.
#[must_use]
pub fn select_unused_images<'a, S: BuildHasher>(
    images: &'a [ImageRecord],
    in_use: &HashSet<Digest, S>,
) -> Vec<&'a ImageRecord> {
    images
        .iter()
        .filter(|img| !img.pinned && !in_use.contains(&img.digest))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn dig(c: char) -> Result<Digest, crate::digest::DigestError> {
        Digest::parse(&format!("sha256:{}", String::from(c).repeat(64)))
    }

    fn img(c: char, pinned: bool) -> Result<ImageRecord, Box<dyn std::error::Error>> {
        Ok(ImageRecord {
            digest: dig(c)?,
            pinned,
            size: 100,
        })
    }

    #[test]
    fn unreferenced_unpinned_image_is_selected() -> TestResult {
        let images = vec![img('a', false)?, img('b', false)?];
        let mut in_use = HashSet::new();
        in_use.insert(dig('a')?);
        let unused = select_unused_images(&images, &in_use);
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].digest, dig('b')?);
        Ok(())
    }

    #[test]
    fn pinned_image_is_never_selected() -> TestResult {
        let images = vec![img('a', true)?];
        let unused = select_unused_images(&images, &HashSet::new());
        assert_eq!(unused, [] as [&ImageRecord; 0]);
        Ok(())
    }

    #[test]
    fn referenced_image_is_retained() -> TestResult {
        let images = vec![img('a', false)?];
        let mut in_use = HashSet::new();
        in_use.insert(dig('a')?);
        assert_eq!(
            select_unused_images(&images, &in_use),
            [] as [&ImageRecord; 0]
        );
        Ok(())
    }

    #[test]
    fn all_unreferenced_unpinned_are_selected_in_order() -> TestResult {
        let images = vec![img('a', false)?, img('b', true)?, img('c', false)?];
        let unused = select_unused_images(&images, &HashSet::new());
        let digs: Vec<_> = unused.iter().map(|i| i.digest.clone()).collect();
        assert_eq!(digs, vec![dig('a')?, dig('c')?]);
        Ok(())
    }

    #[test]
    fn empty_inputs_select_nothing() {
        assert_eq!(
            select_unused_images(&[], &HashSet::new()),
            [] as [&ImageRecord; 0]
        );
    }
}
