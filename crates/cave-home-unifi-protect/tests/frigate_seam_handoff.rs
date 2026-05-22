// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Cross-crate handoff test: the FrigateSeam contract behaves as
// documented in docs/upstream/unifi-protect-frigate-handoff.md.
//
// This test stays inside the protect crate (so it can build without
// pulling cave-home-camera into our dependency graph), but it pins
// the seam shape the camera crate will consume.

use cave_home_unifi_protect::{
    CameraId, FrigateSeam, ProtectCamera, ProtectNvr, ProtectSubsystem, NvrId,
};

#[test]
fn seam_filters_by_subsystem() {
    let mut seam = FrigateSeam::new();
    seam.assign_camera(CameraId::new("g4-doorbell"), ProtectSubsystem::Native);
    seam.assign_camera(CameraId::new("g5-bullet"), ProtectSubsystem::FrigateMl);
    seam.assign_camera(CameraId::new("reolink"), ProtectSubsystem::FrigateOnly);

    let native: Vec<_> = seam.cameras_in(ProtectSubsystem::Native).collect();
    assert_eq!(native.len(), 1);
    let frigate_ml: Vec<_> = seam.cameras_in(ProtectSubsystem::FrigateMl).collect();
    assert_eq!(frigate_ml.len(), 1);
    let frigate_only: Vec<_> = seam.cameras_in(ProtectSubsystem::FrigateOnly).collect();
    assert_eq!(frigate_only.len(), 1);
}

#[test]
fn unknown_camera_returns_none() {
    let seam = FrigateSeam::new();
    // ADR-009 §5.2 handoff doc: unknown cameras MUST yield None — the
    // portal then renders a "setup-required" tile.
    assert!(seam.owner_of(&CameraId::new("never-seen")).is_none());
}

#[test]
fn nvr_camera_can_be_assigned_native() {
    let mut nvr = ProtectNvr::new(NvrId::new("nvr1"), "Ev NVR");
    let cam = ProtectCamera::new(CameraId::new("g4-doorbell"), "Ön kapı");
    nvr.add_camera(cam.clone());

    let mut seam = FrigateSeam::new();
    seam.assign_camera(cam.id.clone(), ProtectSubsystem::Native);

    assert_eq!(seam.owner_of(&cam.id), Some(ProtectSubsystem::Native));
    assert!(nvr.cameras.contains_key(&cam.id));
}
