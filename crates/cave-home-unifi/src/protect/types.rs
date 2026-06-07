// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The UniFi Protect wire DTOs (bootstrap, cameras, events) and their mapping
//! onto the [`cave_home_unifi_protect`] domain model, plus the binary
//! update-WebSocket packet header.
//!
//! Protect rides the console session (it is behind `/proxy/protect`), so its
//! REST responses are plain JSON — the camera array lives inside the `bootstrap`
//! document. The live view is **RTSPS**: every camera channel advertises an
//! `rtspAlias`, and the playable URL is `rtsps://<host>:7441/<alias>?enableSrtp`.
//! The real-time update stream is a **binary** frame protocol (an 8-byte action
//! header + a payload), parsed by [`ProtectPacketHeader`].

use serde::Deserialize;

use cave_home_unifi_protect::{
    CameraId, DetectionEvent, DeviceState, ProtectCamera, RecordingMode, SmartDetectType,
};

use crate::error::UnifiError;

/// The default RTSPS port a UniFi Protect NVR streams on.
pub const RTSPS_PORT: u16 = 7441;

/// The Protect `bootstrap` document (only the parts cave-home reasons about).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireBootstrap {
    /// The NVR record.
    #[serde(default)]
    pub nvr: WireNvr,
    /// Every adopted camera.
    #[serde(default)]
    pub cameras: Vec<WireCamera>,
    /// The opaque update cursor (`lastUpdateId`) the WS uses.
    #[serde(default, rename = "lastUpdateId")]
    pub last_update_id: Option<String>,
}

/// The NVR record inside the bootstrap.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireNvr {
    /// NVR id.
    #[serde(default)]
    pub id: String,
    /// NVR name.
    #[serde(default)]
    pub name: String,
    /// Firmware version.
    #[serde(default)]
    pub version: String,
}

/// A camera's feature flags.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireFeatureFlags {
    /// Has a microphone.
    #[serde(default, rename = "hasMic")]
    pub has_mic: bool,
    /// Has a speaker (two-way talk).
    #[serde(default, rename = "hasSpeaker")]
    pub has_speaker: bool,
    /// Is a doorbell (button + chime).
    #[serde(default, rename = "isDoorbell")]
    pub is_doorbell: bool,
    /// The smart-detect types this hardware can recognise (wire tokens).
    #[serde(default, rename = "smartDetectTypes")]
    pub smart_detect_types: Vec<String>,
}

/// A camera's recording settings.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireRecordingSettings {
    /// The configured mode: `always` / `never` / `detections` / `schedule`.
    #[serde(default)]
    pub mode: String,
}

/// A camera channel (carries the RTSPS alias).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireChannel {
    /// Channel id (0 = high, 1 = medium, 2 = low).
    #[serde(default)]
    pub id: i64,
    /// The RTSPS alias used to build the playable URL.
    #[serde(default, rename = "rtspAlias")]
    pub rtsp_alias: Option<String>,
}

/// A Protect camera (or doorbell).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireCamera {
    /// Stable NVR id.
    #[serde(default)]
    pub id: String,
    /// Household name.
    #[serde(default)]
    pub name: String,
    /// MAC.
    #[serde(default)]
    pub mac: String,
    /// Camera type string ("UVC G4 Doorbell", ...).
    #[serde(default, rename = "type")]
    pub cam_type: String,
    /// Connection state ("CONNECTED" / "DISCONNECTED").
    #[serde(default)]
    pub state: Option<String>,
    /// Whether currently connected (newer field).
    #[serde(default, rename = "isConnected")]
    pub is_connected: Option<bool>,
    /// Feature flags.
    #[serde(default, rename = "featureFlags")]
    pub feature_flags: WireFeatureFlags,
    /// Recording settings.
    #[serde(default, rename = "recordingSettings")]
    pub recording_settings: WireRecordingSettings,
    /// Channels (for the RTSPS alias).
    #[serde(default)]
    pub channels: Vec<WireChannel>,
}

/// Map the wire recording-mode token to the domain [`RecordingMode`].
#[must_use]
pub fn recording_mode(mode: &str) -> RecordingMode {
    match mode {
        "always" => RecordingMode::Always,
        "never" => RecordingMode::Never,
        "schedule" => RecordingMode::Schedule,
        // "detections" and anything unknown default to detection-triggered.
        _ => RecordingMode::Detections,
    }
}

impl WireCamera {
    /// Whether the camera reports connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected
            .unwrap_or_else(|| self.state.as_deref() == Some("CONNECTED"))
    }

    /// Whether this camera is a doorbell (flag or type string).
    #[must_use]
    pub fn is_doorbell(&self) -> bool {
        self.feature_flags.is_doorbell || self.cam_type.contains("Doorbell")
    }

    /// The first usable RTSPS alias, if any.
    #[must_use]
    pub fn rtsp_alias(&self) -> Option<&str> {
        self.channels
            .iter()
            .find_map(|c| c.rtsp_alias.as_deref().filter(|s| !s.is_empty()))
    }

    /// The playable RTSPS live-stream URL for this camera on `host`, if it
    /// advertises an alias.
    #[must_use]
    pub fn live_rtsps_url(&self, host: &str) -> Option<String> {
        self.rtsp_alias()
            .map(|alias| format!("rtsps://{host}:{RTSPS_PORT}/{alias}?enableSrtp"))
    }

    /// Lower to the domain [`ProtectCamera`].
    #[must_use]
    pub fn into_domain(self) -> ProtectCamera {
        let id = CameraId::new(self.id.clone());
        let name = if self.name.is_empty() {
            self.id.clone()
        } else {
            self.name.clone()
        };
        let mut cam = ProtectCamera::new(id, name, self.mac.clone())
            .with_recording_mode(recording_mode(&self.recording_settings.mode))
            .with_state(if self.is_connected() {
                DeviceState::Online
            } else {
                DeviceState::Offline
            });
        if self.is_doorbell() {
            cam = cam.as_doorbell();
        }
        cam.has_mic = self.feature_flags.has_mic || cam.has_mic;
        cam.has_speaker = self.feature_flags.has_speaker || cam.has_speaker;
        for t in &self.feature_flags.smart_detect_types {
            if let Some(feature) = SmartDetectType::parse(t) {
                cam = cam.with_feature(feature);
            }
        }
        cam
    }
}

/// A Protect event (`/api/events`): a smart detection, a ring, a motion.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireEvent {
    /// Event id.
    #[serde(default)]
    pub id: String,
    /// Event type ("smartDetectZone", "ring", "motion").
    #[serde(default, rename = "type")]
    pub event_type: String,
    /// The camera id the event belongs to.
    #[serde(default)]
    pub camera: Option<String>,
    /// Detection confidence 0–100.
    #[serde(default)]
    pub score: Option<u8>,
    /// Start time (unix milliseconds).
    #[serde(default)]
    pub start: Option<u64>,
    /// End time (unix milliseconds); absent while the event is live.
    #[serde(default)]
    pub end: Option<u64>,
    /// The smart-detect types in this event (wire tokens).
    #[serde(default, rename = "smartDetectTypes")]
    pub smart_detect_types: Vec<String>,
    /// Thumbnail id, if any.
    #[serde(default)]
    pub thumbnail: Option<String>,
}

impl WireEvent {
    /// Whether this is a doorbell ring event.
    #[must_use]
    pub fn is_ring(&self) -> bool {
        self.event_type == "ring"
    }

    /// Lower a smart-detect event to a domain [`DetectionEvent`]. Returns
    /// `None` for events without a camera (which the domain model requires).
    #[must_use]
    pub fn into_detection(self) -> Option<DetectionEvent> {
        let camera = self.camera.clone()?;
        let score = self.score.unwrap_or(0);
        let start = self.start.unwrap_or(0);
        let mut ev = DetectionEvent::new(camera, score, start);
        if let Some(end) = self.end {
            ev = ev.ended_at(end);
        }
        if let Some(thumb) = self.thumbnail.filter(|s| !s.is_empty()) {
            ev = ev.with_thumbnail(thumb);
        }
        for t in &self.smart_detect_types {
            if let Some(feature) = SmartDetectType::parse(t) {
                ev = ev.with_type(feature);
            }
        }
        Some(ev)
    }
}

/// The 8-byte header of a Protect update-WebSocket packet.
///
/// The Protect WS frames are binary: each logical message is two packets (an
/// "action" packet then a "data" packet), and each packet begins with this
/// header. cave-home parses the header to size + route the payload; the payload
/// itself (optionally deflate-compressed JSON) is handled by the WS engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectPacketHeader {
    /// Packet type: 1 = action frame, 2 = payload/data frame.
    pub packet_type: u8,
    /// Payload format: 1 = JSON, 2 = UTF-8 string, 3 = node buffer.
    pub payload_format: u8,
    /// Whether the payload is zlib-deflated.
    pub deflated: bool,
    /// The payload length in bytes (big-endian u32 at offset 4).
    pub payload_size: u32,
}

impl ProtectPacketHeader {
    /// The fixed header length.
    pub const LEN: usize = 8;

    /// Parse the 8-byte header from the start of a frame.
    ///
    /// # Errors
    /// [`UnifiError::WebSocket`] if fewer than 8 bytes are available.
    pub fn parse(bytes: &[u8]) -> crate::Result<Self> {
        if bytes.len() < Self::LEN {
            return Err(UnifiError::WebSocket(format!(
                "protect packet header needs {} bytes, got {}",
                Self::LEN,
                bytes.len()
            )));
        }
        let payload_size = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        Ok(Self {
            packet_type: bytes[0],
            payload_format: bytes[1],
            deflated: bytes[2] != 0,
            payload_size,
        })
    }

    /// Whether this is an action frame.
    #[must_use]
    pub fn is_action(&self) -> bool {
        self.packet_type == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_mode_mapping() {
        assert_eq!(recording_mode("always"), RecordingMode::Always);
        assert_eq!(recording_mode("never"), RecordingMode::Never);
        assert_eq!(recording_mode("schedule"), RecordingMode::Schedule);
        assert_eq!(recording_mode("detections"), RecordingMode::Detections);
        assert_eq!(recording_mode("???"), RecordingMode::Detections);
    }

    #[test]
    fn doorbell_camera_maps_to_domain_with_audio_and_features() {
        let w: WireCamera = serde_json::from_str(
            r#"{"id":"c1","name":"Front door","mac":"ab:cd","type":"UVC G4 Doorbell",
                "state":"CONNECTED",
                "featureFlags":{"hasMic":true,"hasSpeaker":true,"isDoorbell":true,
                                "smartDetectTypes":["person","package"]},
                "recordingSettings":{"mode":"detections"},
                "channels":[{"id":0,"rtspAlias":"abcDEF123"}]}"#,
        )
        .unwrap();
        assert!(w.is_doorbell());
        assert!(w.is_connected());
        let cam = w.into_domain();
        assert_eq!(cam.name, "Front door");
        assert!(cam.is_doorbell);
        assert!(cam.has_speaker);
        assert!(cam.is_online());
        assert!(cam.supports(SmartDetectType::Person));
        assert!(cam.supports(SmartDetectType::Package));
    }

    #[test]
    fn live_rtsps_url_built_from_channel_alias() {
        let w: WireCamera = serde_json::from_str(
            r#"{"id":"c1","name":"Drive","mac":"m","channels":[
                {"id":0,"rtspAlias":""},{"id":1,"rtspAlias":"XyZ987"}]}"#,
        )
        .unwrap();
        assert_eq!(
            w.live_rtsps_url("10.0.0.3"),
            Some("rtsps://10.0.0.3:7441/XyZ987?enableSrtp".to_string())
        );
    }

    #[test]
    fn camera_without_alias_has_no_url() {
        let w: WireCamera =
            serde_json::from_str(r#"{"id":"c","name":"n","mac":"m","channels":[]}"#).unwrap();
        assert_eq!(w.live_rtsps_url("h"), None);
    }

    #[test]
    fn disconnected_camera_is_offline() {
        let w: WireCamera = serde_json::from_str(
            r#"{"id":"c","name":"n","mac":"m","isConnected":false}"#,
        )
        .unwrap();
        assert_eq!(w.into_domain().state, DeviceState::Offline);
    }

    #[test]
    fn bootstrap_parses_cameras_and_nvr() {
        let b: WireBootstrap = serde_json::from_str(
            r#"{"lastUpdateId":"u-1","nvr":{"id":"nvr1","name":"Home NVR","version":"4.0.0"},
                "cameras":[{"id":"c1","name":"Cam","mac":"m","state":"CONNECTED"}]}"#,
        )
        .unwrap();
        assert_eq!(b.nvr.name, "Home NVR");
        assert_eq!(b.cameras.len(), 1);
        assert_eq!(b.last_update_id.as_deref(), Some("u-1"));
    }

    #[test]
    fn event_smart_detect_maps_to_detection() {
        let e: WireEvent = serde_json::from_str(
            r#"{"id":"e1","type":"smartDetectZone","camera":"c1","score":92,
                "start":1717000000000,"end":1717000005000,
                "smartDetectTypes":["person"],"thumbnail":"t1"}"#,
        )
        .unwrap();
        let det = e.into_detection().unwrap();
        assert!(det.has_type(SmartDetectType::Person));
        assert!(!det.is_active()); // has an end
    }

    #[test]
    fn event_without_camera_is_none() {
        let e: WireEvent =
            serde_json::from_str(r#"{"id":"e","type":"motion","score":10}"#).unwrap();
        assert!(e.into_detection().is_none());
    }

    #[test]
    fn ring_event_flagged() {
        let e: WireEvent =
            serde_json::from_str(r#"{"id":"e","type":"ring","camera":"c1"}"#).unwrap();
        assert!(e.is_ring());
    }

    #[test]
    fn protect_packet_header_parses_be_size() {
        // type=1 action, format=1 json, deflated=1, size=0x00000123 = 291
        let bytes = [1u8, 1, 1, 0, 0x00, 0x00, 0x01, 0x23, 0xAA, 0xBB];
        let h = ProtectPacketHeader::parse(&bytes).unwrap();
        assert_eq!(h.packet_type, 1);
        assert!(h.is_action());
        assert_eq!(h.payload_format, 1);
        assert!(h.deflated);
        assert_eq!(h.payload_size, 291);
    }

    #[test]
    fn protect_packet_header_rejects_short() {
        assert!(ProtectPacketHeader::parse(&[1, 2, 3]).is_err());
    }
}
