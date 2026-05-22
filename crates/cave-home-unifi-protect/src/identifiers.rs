// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifiprotect/data.py
//                            + uiprotect.data.Camera.id / NVR.id / Event.id

use serde::{Deserialize, Serialize};

/// UniFi Protect camera identifier (HA: `camera.id` — 24-char hex GUID).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CameraId(String);

impl CameraId {
    /// Construct from any string.
    #[must_use]
    pub fn new<S: Into<String>>(raw: S) -> Self {
        Self(raw.into())
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CameraId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// UniFi NVR identifier (HA: `nvr.id`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NvrId(String);

impl NvrId {
    /// Construct from any string.
    #[must_use]
    pub fn new<S: Into<String>>(raw: S) -> Self {
        Self(raw.into())
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NvrId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// UniFi Protect event identifier (HA: `event.id`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(String);

impl EventId {
    /// Construct from any string.
    #[must_use]
    pub fn new<S: Into<String>>(raw: S) -> Self {
        Self(raw.into())
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_id_round_trip() {
        let id = CameraId::new("64xxx");
        assert_eq!(id.as_str(), "64xxx");
        assert_eq!(id.to_string(), "64xxx");
    }

    #[test]
    fn nvr_id_round_trip() {
        assert_eq!(NvrId::new("nvr").as_str(), "nvr");
    }

    #[test]
    fn event_id_round_trip() {
        assert_eq!(EventId::new("evt").as_str(), "evt");
    }
}
