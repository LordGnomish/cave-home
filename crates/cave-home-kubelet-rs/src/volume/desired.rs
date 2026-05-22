// SPDX-License-Identifier: Apache-2.0
//! `DesiredStateOfWorld`.
//!
//! Hand-port of `pkg/kubelet/volumemanager/cache/desired_state_of_world.go`.

use std::collections::HashMap;

use parking_lot::Mutex;

use crate::api::{PodUid, Volume};

/// What the kubelet *wants* to be mounted.
#[derive(Default)]
pub struct DesiredStateOfWorld {
    /// pod_uid -> [Volume]
    inner: Mutex<HashMap<PodUid, Vec<Volume>>>,
}

impl DesiredStateOfWorld {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the volumes a pod wants.
    pub fn add_pod(&self, uid: PodUid, volumes: Vec<Volume>) {
        self.inner.lock().insert(uid, volumes);
    }

    /// Drop a pod from the desired state.
    pub fn remove_pod(&self, uid: &PodUid) {
        self.inner.lock().remove(uid);
    }

    /// Snapshot of (pod_uid, volume) pairs.
    pub fn snapshot(&self) -> Vec<(PodUid, Volume)> {
        let g = self.inner.lock();
        let mut out = Vec::new();
        for (uid, vols) in g.iter() {
            for v in vols {
                out.push((uid.clone(), v.clone()));
            }
        }
        out
    }

    /// True iff this pod-uid is registered.
    pub fn has_pod(&self, uid: &PodUid) -> bool {
        self.inner.lock().contains_key(uid)
    }

    /// Number of registered pods.
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}
