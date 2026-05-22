// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// RED-phase integration tests for cave-home-unifi-protect.
//
// Upstream pin: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//               (tag 2026.5.2) :: homeassistant/components/unifiprotect/

use cave_home_unifi_protect::{
    CameraChannel, CameraId, EventId, EventKind, FrigateSeam, NvrConfig, NvrId,
    ProtectCamera, ProtectClient, ProtectError, ProtectEvent, ProtectNvr,
    ProtectSubsystem, friendly_camera_label, MIN_PROTECT_VERSION,
};

#[test]
fn nvr_config_defaults_to_https_443() {
    let cfg = NvrConfig::new("nvr.local", "admin", "secret");
    assert_eq!(cfg.host, "nvr.local");
    assert_eq!(cfg.port, 443);
    assert!(!cfg.verify_ssl);
    assert!(!cfg.disable_rtsp);
}

#[test]
fn nvr_config_chainable_overrides() {
    let cfg = NvrConfig::new("nvr.local", "admin", "secret")
        .with_port(8443)
        .with_verify_ssl(true)
        .with_disable_rtsp(true)
        .with_override_connection_host("nvr.lan");
    assert_eq!(cfg.port, 8443);
    assert!(cfg.verify_ssl);
    assert!(cfg.disable_rtsp);
    assert_eq!(cfg.override_connection_host.as_deref(), Some("nvr.lan"));
}

#[test]
fn min_protect_version_is_v6() {
    // Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
    // homeassistant/components/unifiprotect/const.py :: MIN_REQUIRED_PROTECT_V
    assert_eq!(MIN_PROTECT_VERSION, "6.0.0");
}

#[test]
fn event_kind_strings() {
    // Source: uiprotect.data.EventType (motion, ring, smartDetectZone, etc.)
    assert_eq!(EventKind::Motion.as_str(), "motion");
    assert_eq!(EventKind::Ring.as_str(), "ring");
    assert_eq!(EventKind::SmartDetectZone.as_str(), "smartDetectZone");
    assert_eq!(EventKind::SmartDetectLine.as_str(), "smartDetectLine");
    assert_eq!(EventKind::FingerprintIdentified.as_str(), "identified");
    assert_eq!(EventKind::FingerprintNotIdentified.as_str(), "not_identified");
    assert_eq!(EventKind::NfcScanned.as_str(), "scanned");
    assert_eq!(EventKind::VehicleDetected.as_str(), "detected");
}

#[test]
fn event_kind_parse_round_trip() {
    for variant in EventKind::all() {
        assert_eq!(EventKind::parse(variant.as_str()), Some(variant));
    }
    assert_eq!(EventKind::parse("nonsense"), None);
}

#[test]
fn protect_camera_constructs_with_zero_channels() {
    let id = CameraId::new("64xxx");
    let cam = ProtectCamera::new(id.clone(), "Ön kapı");
    assert_eq!(cam.id, id);
    assert_eq!(cam.label, "Ön kapı");
    assert!(cam.channels.is_empty());
    assert!(!cam.is_doorbell);
    assert!(!cam.has_motion);
}

#[test]
fn protect_camera_with_channels() {
    let mut cam = ProtectCamera::new(CameraId::new("64xxx"), "Salon");
    cam.channels.push(CameraChannel {
        idx: 0,
        name: "Yüksek".into(),
        width: 3840,
        height: 2160,
        fps: 25,
        bitrate: 8_000_000,
    });
    assert_eq!(cam.channels.len(), 1);
    assert_eq!(cam.channels[0].width, 3840);
}

#[test]
fn friendly_label_drops_camera_jargon() {
    // ADR-007: portal must say "Kamera" before the user-set name; we
    // never expose the camera_id GUID as the default label.
    assert_eq!(friendly_camera_label("Salon"), "Salon kamerası");
    assert_eq!(friendly_camera_label("Ön kapı"), "Ön kapı kamerası");
    // Falsy fallback for empty names
    assert_eq!(friendly_camera_label(""), "Adsız kamera");
}

#[test]
fn protect_event_construction() {
    let e = ProtectEvent {
        id: EventId::new("evt-1"),
        camera: CameraId::new("64xxx"),
        kind: EventKind::Motion,
        score: 42,
        started_at_ms: 1_700_000_000_000,
        ended_at_ms: Some(1_700_000_010_000),
    };
    assert_eq!(e.score, 42);
    assert!(e.is_active() == false); // ended_at_ms is Some
    let active = ProtectEvent {
        id: EventId::new("evt-2"),
        camera: CameraId::new("64xxx"),
        kind: EventKind::Ring,
        score: 0,
        started_at_ms: 1_700_000_000_000,
        ended_at_ms: None,
    };
    assert!(active.is_active());
}

#[test]
fn protect_nvr_construction() {
    let nvr = ProtectNvr::new(NvrId::new("nvr-1"), "Ev NVR");
    assert_eq!(nvr.label, "Ev NVR");
    assert!(nvr.cameras.is_empty());
}

#[test]
fn protect_client_unauthenticated_initially() {
    let cfg = NvrConfig::new("nvr.local", "admin", "secret");
    let c = ProtectClient::new(cfg);
    assert!(!c.is_authenticated());
}

#[tokio::test]
async fn protect_client_login_against_offline_host_errors() {
    let cfg = NvrConfig::new("127.0.0.1", "admin", "secret").with_port(1);
    let mut c = ProtectClient::new(cfg);
    let err = c.login().await.unwrap_err();
    assert!(matches!(err, ProtectError::Connect(_) | ProtectError::Timeout));
}

#[test]
fn frigate_seam_records_handoff() {
    // The Frigate ↔ Protect seam is a documented contract: which detector
    // owns which camera, which event stream the automation reads.
    let mut seam = FrigateSeam::new();
    seam.assign_camera(
        CameraId::new("64xxx"),
        ProtectSubsystem::Native,
    );
    seam.assign_camera(
        CameraId::new("rtsp-only-001"),
        ProtectSubsystem::FrigateMl,
    );
    assert_eq!(
        seam.owner_of(&CameraId::new("64xxx")),
        Some(ProtectSubsystem::Native)
    );
    assert_eq!(
        seam.owner_of(&CameraId::new("rtsp-only-001")),
        Some(ProtectSubsystem::FrigateMl)
    );
    assert_eq!(seam.owner_of(&CameraId::new("missing")), None);
}

#[test]
fn protect_event_in_smart_detection_family() {
    assert!(EventKind::SmartDetectZone.is_smart_detection());
    assert!(EventKind::SmartDetectLine.is_smart_detection());
    assert!(!EventKind::Motion.is_smart_detection());
    assert!(!EventKind::Ring.is_smart_detection());
}
