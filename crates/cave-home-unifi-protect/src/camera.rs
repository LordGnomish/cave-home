// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifiprotect/camera.py
//                            + uiprotect.data.Camera + CameraChannel
//
// HA's `camera.py` is 278 lines wiring `ProtectCamera` entity surfaces
// (stream URLs, channel selection, RTSP repair issues). cave-home Phase 1
// ports the data shape — `ProtectCamera` + `CameraChannel` — that the
// portal needs to render the camera tile. Wire-side stream URL
// resolution is a Phase 2 ticket.

use serde::{Deserialize, Serialize};

use crate::identifiers::CameraId;

/// One camera stream channel (HA: `uiprotect.data.CameraChannel`).
///
/// Cameras typically expose three channels — high (4K), medium (1080p),
/// low (480p). The portal picks one by default; advanced users can switch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraChannel {
    /// Channel index in the camera's channel list.
    pub idx: u32,
    /// Friendly channel name (e.g. "Yüksek", "Orta", "Düşük").
    pub name: String,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frames per second.
    pub fps: u32,
    /// Encoded bitrate (bits/sec).
    pub bitrate: u32,
}

/// A UniFi Protect camera entity.
///
/// Source: HA `unifiprotect/camera.py` + the `uiprotect.data.Camera`
/// pydantic model. Phase 1 fields:
///   - id, label
///   - channels[]
///   - is_doorbell — drives whether the portal shows the doorbell-ring
///     event tile
///   - has_motion — drives motion-event subscription
///
/// Phase 2 ticket: full Camera model parity (PTZ, AI smart-detect lines,
/// privacy mask, micro-detect, etc.).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectCamera {
    /// Camera GUID.
    pub id: CameraId,
    /// User-set name from the NVR (or model name if unset).
    pub label: String,
    /// Available stream channels.
    pub channels: Vec<CameraChannel>,
    /// True if this is a G4 Doorbell / G5 Doorbell Pro (the camera
    /// exposes a ring button).
    pub is_doorbell: bool,
    /// True if the camera supports motion detection.
    pub has_motion: bool,
}

impl ProtectCamera {
    /// Construct an empty camera (no channels).
    #[must_use]
    pub fn new(id: CameraId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            channels: Vec::new(),
            is_doorbell: false,
            has_motion: false,
        }
    }

    /// Best-fit channel by total pixels.
    #[must_use]
    pub fn highest_resolution_channel(&self) -> Option<&CameraChannel> {
        self.channels
            .iter()
            .max_by_key(|c| (c.width as u64) * (c.height as u64))
    }
}

/// ADR-007 home-world camera label. Strips the controller's "Camera 1"
/// jargon by tacking "kamerası" onto the user-set name. Empty / unset
/// names fall back to "Adsız kamera".
#[must_use]
pub fn friendly_camera_label(user_name: &str) -> String {
    let trimmed = user_name.trim();
    if trimmed.is_empty() {
        "Adsız kamera".to_string()
    } else {
        format!("{trimmed} kamerası")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highest_resolution_picks_4k() {
        let mut c = ProtectCamera::new(CameraId::new("x"), "Salon");
        c.channels.push(CameraChannel {
            idx: 0,
            name: "Düşük".into(),
            width: 640,
            height: 480,
            fps: 15,
            bitrate: 200_000,
        });
        c.channels.push(CameraChannel {
            idx: 1,
            name: "Yüksek".into(),
            width: 3840,
            height: 2160,
            fps: 25,
            bitrate: 8_000_000,
        });
        let best = c.highest_resolution_channel().unwrap();
        assert_eq!(best.width, 3840);
    }

    #[test]
    fn friendly_label_appends_kamerasi() {
        assert_eq!(friendly_camera_label("Salon"), "Salon kamerası");
    }

    #[test]
    fn friendly_label_empty_fallback() {
        assert_eq!(friendly_camera_label("   "), "Adsız kamera");
    }
}
