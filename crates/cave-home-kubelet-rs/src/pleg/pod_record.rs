// SPDX-License-Identifier: Apache-2.0
//! `PodRecord` — last-seen snapshot kept by the GenericPLEG.
//!
//! Hand-port of `pkg/kubelet/pleg/generic.go::podRecord`.

use std::collections::HashMap;

use crate::api::PodUid;
use crate::cri::types::ContainerState;

/// Per-container state remembered by the PLEG between relists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerSnapshot {
    pub id: String,
    pub state: ContainerState,
}

/// Per-pod snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodRecord {
    pub uid: PodUid,
    pub containers: Vec<ContainerSnapshot>,
}

impl PodRecord {
    /// Find a container by ID inside the snapshot, if present.
    #[must_use]
    pub fn container(&self, id: &str) -> Option<&ContainerSnapshot> {
        self.containers.iter().find(|c| c.id == id)
    }
}

/// Keyed cache of pod records.
pub type PodRecords = HashMap<PodUid, PodRecord>;
