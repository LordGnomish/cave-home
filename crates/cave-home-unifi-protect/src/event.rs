// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifiprotect/event.py
//                            + uiprotect.data.EventType enum

use serde::{Deserialize, Serialize};

use crate::identifiers::{CameraId, EventId};

/// UniFi Protect event type (HA: `uiprotect.data.EventType`).
///
/// The full uiprotect EventType enum has ~25 variants; cave-home Phase 1
/// surfaces the eight that the automation engine + portal grandma-view
/// care about. Phase 2 ticket: complete EventType parity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    /// Plain motion detection.
    Motion,
    /// Doorbell button press.
    Ring,
    /// Smart-detect crossed an in-zone polygon.
    SmartDetectZone,
    /// Smart-detect crossed a line.
    SmartDetectLine,
    /// Fingerprint reader recognised a known print.
    FingerprintIdentified,
    /// Fingerprint reader saw an unknown print.
    FingerprintNotIdentified,
    /// NFC reader scanned a tag.
    NfcScanned,
    /// Vehicle entered a smart-detect zone.
    VehicleDetected,
}

impl EventKind {
    /// String form as shipped by the NVR WebSocket.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Motion => "motion",
            Self::Ring => "ring",
            Self::SmartDetectZone => "smartDetectZone",
            Self::SmartDetectLine => "smartDetectLine",
            Self::FingerprintIdentified => crate::const_table::EVENT_TYPE_FINGERPRINT_IDENTIFIED,
            Self::FingerprintNotIdentified => {
                crate::const_table::EVENT_TYPE_FINGERPRINT_NOT_IDENTIFIED
            }
            Self::NfcScanned => crate::const_table::EVENT_TYPE_NFC_SCANNED,
            Self::VehicleDetected => crate::const_table::EVENT_TYPE_VEHICLE_DETECTED,
        }
    }

    /// Parse the wire form back into a variant.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "motion" => Self::Motion,
            "ring" => Self::Ring,
            "smartDetectZone" => Self::SmartDetectZone,
            "smartDetectLine" => Self::SmartDetectLine,
            "identified" => Self::FingerprintIdentified,
            "not_identified" => Self::FingerprintNotIdentified,
            "scanned" => Self::NfcScanned,
            "detected" => Self::VehicleDetected,
            _ => return None,
        })
    }

    /// True if this is an AI-driven detection (vs raw motion / ring).
    #[must_use]
    pub fn is_smart_detection(self) -> bool {
        matches!(self, Self::SmartDetectZone | Self::SmartDetectLine)
    }

    /// Enumerate every variant.
    #[must_use]
    pub fn all() -> [Self; 8] {
        [
            Self::Motion,
            Self::Ring,
            Self::SmartDetectZone,
            Self::SmartDetectLine,
            Self::FingerprintIdentified,
            Self::FingerprintNotIdentified,
            Self::NfcScanned,
            Self::VehicleDetected,
        ]
    }
}

/// A discrete UniFi Protect event.
///
/// Source: HA `unifiprotect/event.py` event-entity update path +
/// `uiprotect.data.Event` model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectEvent {
    /// Event GUID.
    pub id: EventId,
    /// Camera GUID that emitted this event.
    pub camera: CameraId,
    /// Event family.
    pub kind: EventKind,
    /// Confidence score (0–100; only meaningful for smart detections).
    pub score: u32,
    /// Start time, Unix epoch milliseconds (HA: `event.start`).
    pub started_at_ms: i64,
    /// End time, Unix epoch milliseconds. `None` while the event is
    /// still firing.
    pub ended_at_ms: Option<i64>,
}

impl ProtectEvent {
    /// True if the event is still active (no end time yet).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.ended_at_ms.is_none()
    }

    /// Event duration in milliseconds, or 0 if still active.
    #[must_use]
    pub fn duration_ms(&self) -> i64 {
        self.ended_at_ms
            .map_or(0, |end| (end - self.started_at_ms).max(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_round_trip_all() {
        for k in EventKind::all() {
            assert_eq!(EventKind::parse(k.as_str()), Some(k));
        }
    }

    #[test]
    fn smart_detection_is_smart() {
        assert!(EventKind::SmartDetectZone.is_smart_detection());
        assert!(EventKind::SmartDetectLine.is_smart_detection());
    }

    #[test]
    fn motion_is_not_smart() {
        assert!(!EventKind::Motion.is_smart_detection());
    }

    #[test]
    fn duration_zero_while_active() {
        let e = ProtectEvent {
            id: EventId::new("e"),
            camera: CameraId::new("c"),
            kind: EventKind::Motion,
            score: 50,
            started_at_ms: 1_000,
            ended_at_ms: None,
        };
        assert_eq!(e.duration_ms(), 0);
        assert!(e.is_active());
    }

    #[test]
    fn duration_when_ended() {
        let e = ProtectEvent {
            id: EventId::new("e"),
            camera: CameraId::new("c"),
            kind: EventKind::Motion,
            score: 50,
            started_at_ms: 1_000,
            ended_at_ms: Some(2_500),
        };
        assert_eq!(e.duration_ms(), 1_500);
        assert!(!e.is_active());
    }
}
