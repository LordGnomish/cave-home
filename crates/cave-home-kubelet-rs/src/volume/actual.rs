// SPDX-License-Identifier: Apache-2.0
//! `ActualStateOfWorld`.
//!
//! Hand-port of `pkg/kubelet/volumemanager/cache/actual_state_of_world.go`.

use std::collections::HashMap;
use std::path::PathBuf;

use parking_lot::Mutex;

use crate::api::PodUid;

/// One entry per (pod_uid, volume_name) currently mounted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountedVolume {
    pub pod_uid: PodUid,
    pub volume_name: String,
    pub host_path: PathBuf,
}

/// What is actually mounted right now.
#[derive(Default)]
pub struct ActualStateOfWorld {
    inner: Mutex<HashMap<(PodUid, String), MountedVolume>>,
}

impl ActualStateOfWorld {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_mounted(&self, m: MountedVolume) {
        self.inner
            .lock()
            .insert((m.pod_uid.clone(), m.volume_name.clone()), m);
    }

    pub fn remove_mounted(&self, uid: &PodUid, name: &str) {
        self.inner.lock().remove(&(uid.clone(), name.into()));
    }

    pub fn is_mounted(&self, uid: &PodUid, name: &str) -> bool {
        self.inner.lock().contains_key(&(uid.clone(), name.into()))
    }

    pub fn snapshot(&self) -> Vec<MountedVolume> {
        self.inner.lock().values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }

    pub fn get_host_path(&self, uid: &PodUid, name: &str) -> Option<PathBuf> {
        self.inner
            .lock()
            .get(&(uid.clone(), name.into()))
            .map(|m| m.host_path.clone())
    }
}
