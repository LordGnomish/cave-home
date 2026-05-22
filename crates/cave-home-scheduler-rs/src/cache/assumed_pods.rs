// SPDX-License-Identifier: Apache-2.0
//! Tracker for pods the scheduler has tentatively bound (assumed)
//! but not yet observed via the informer reflection.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/backend/cache/cache.go::Cache.assumePod

use std::collections::HashMap;

use crate::types::Pod;

/// Upstream: `Cache.assumedPods` map.
#[derive(Default, Debug, Clone)]
pub struct AssumedPodTracker {
    pods: HashMap<String, Pod>,
}

impl AssumedPodTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, uid: String, pod: Pod) {
        self.pods.insert(uid, pod);
    }

    #[must_use]
    pub fn contains(&self, uid: &str) -> bool {
        self.pods.contains_key(uid)
    }

    pub fn remove(&mut self, uid: &str) -> Option<Pod> {
        self.pods.remove(uid)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pods.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pods.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ObjectMeta, Pod};

    fn pod(uid: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                uid: uid.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn empty_tracker_contains_nothing() {
        let t = AssumedPodTracker::new();
        assert!(t.is_empty());
        assert!(!t.contains("alpha"));
    }

    #[test]
    fn insert_then_contains() {
        let mut t = AssumedPodTracker::new();
        t.insert("alpha".into(), pod("alpha"));
        assert_eq!(t.len(), 1);
        assert!(t.contains("alpha"));
    }

    #[test]
    fn remove_returns_the_pod() {
        let mut t = AssumedPodTracker::new();
        t.insert("alpha".into(), pod("alpha"));
        let p = t.remove("alpha").unwrap();
        assert_eq!(p.metadata.uid, "alpha");
        assert!(t.is_empty());
    }
}
