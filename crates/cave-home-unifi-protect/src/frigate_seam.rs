// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// The Frigate ↔ UniFi Protect seam.
//
// ADR-009 §5.2: "Where a user runs both UniFi Protect and Frigate, the
// two render through the same camera pillar surface in the Portal."
// ADR-009 open-question 2: "do automation triggers fire from the
// underlying inference (Frigate detector vs UniFi smart detection)
// transparently, or does the user pick? Defer to ADR-014."
//
// The `FrigateSeam` is the cave-home-side decision table. For every
// camera the portal renders, the seam records which subsystem owns:
//   - the live stream (RTSP from Protect or from a generic camera)
//   - the AI inference (Protect smart-detect, or Frigate ML pipeline)
// The portal asks `seam.owner_of(camera_id)` to decide which event
// stream to subscribe to and which thumbnail endpoint to fetch.
//
// See `docs/upstream/unifi-protect-frigate-handoff.md` for the full
// contract.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::identifiers::CameraId;

/// Which subsystem owns AI inference + event-stream for a camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtectSubsystem {
    /// UniFi Protect drives both stream and AI detection. Suitable for
    /// G4 / G5 cameras adopted by a Protect NVR with smart-detect enabled.
    Native,
    /// UniFi Protect drives the stream (camera is adopted) but Frigate
    /// runs the ML inference. User opted into Frigate's detector model
    /// for finer-grained labels.
    FrigateMl,
    /// Camera is RTSP-only (e.g. a third-party Reolink, not adopted by
    /// any Protect NVR). Frigate handles everything.
    FrigateOnly,
}

/// Per-camera ownership table.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct FrigateSeam {
    cameras: HashMap<CameraId, ProtectSubsystem>,
}

impl FrigateSeam {
    /// Construct an empty seam.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Assign a camera to a subsystem.
    pub fn assign_camera(&mut self, id: CameraId, subsystem: ProtectSubsystem) {
        self.cameras.insert(id, subsystem);
    }

    /// Look up which subsystem owns a camera. `None` means "camera
    /// unknown to the seam" — the portal must surface a "configure
    /// camera ownership" issue.
    #[must_use]
    pub fn owner_of(&self, id: &CameraId) -> Option<ProtectSubsystem> {
        self.cameras.get(id).copied()
    }

    /// Count cameras tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cameras.len()
    }

    /// True if the seam tracks no cameras.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cameras.is_empty()
    }

    /// Filter cameras by subsystem.
    pub fn cameras_in(
        &self,
        subsystem: ProtectSubsystem,
    ) -> impl Iterator<Item = &CameraId> + '_ {
        self.cameras
            .iter()
            .filter(move |(_, sys)| **sys == subsystem)
            .map(|(id, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_seam() {
        let s = FrigateSeam::new();
        assert!(s.is_empty());
        assert_eq!(s.owner_of(&CameraId::new("missing")), None);
    }

    #[test]
    fn assignment_round_trip() {
        let mut s = FrigateSeam::new();
        s.assign_camera(CameraId::new("a"), ProtectSubsystem::Native);
        s.assign_camera(CameraId::new("b"), ProtectSubsystem::FrigateMl);
        s.assign_camera(CameraId::new("c"), ProtectSubsystem::FrigateOnly);
        assert_eq!(s.owner_of(&CameraId::new("a")), Some(ProtectSubsystem::Native));
        assert_eq!(s.owner_of(&CameraId::new("b")), Some(ProtectSubsystem::FrigateMl));
        assert_eq!(s.owner_of(&CameraId::new("c")), Some(ProtectSubsystem::FrigateOnly));
    }

    #[test]
    fn filter_by_subsystem() {
        let mut s = FrigateSeam::new();
        s.assign_camera(CameraId::new("a"), ProtectSubsystem::Native);
        s.assign_camera(CameraId::new("b"), ProtectSubsystem::Native);
        s.assign_camera(CameraId::new("c"), ProtectSubsystem::FrigateMl);
        let native: Vec<_> = s.cameras_in(ProtectSubsystem::Native).collect();
        assert_eq!(native.len(), 2);
        let frigate: Vec<_> = s.cameras_in(ProtectSubsystem::FrigateMl).collect();
        assert_eq!(frigate.len(), 1);
    }
}
