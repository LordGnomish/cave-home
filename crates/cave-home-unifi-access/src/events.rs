// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi_access/coordinator.py
//                            _handle_doorbell / _handle_remote_view /
//                            _handle_insights_add / _handle_logs_add

use serde::{Deserialize, Serialize};

use crate::door::DoorId;

/// Category of a door event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DoorEventCategory {
    /// Doorbell button / remote-view press.
    Doorbell,
    /// Access grant / deny via card / mobile / fingerprint.
    Access,
}

impl DoorEventCategory {
    /// Wire-form string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Doorbell => "doorbell",
            Self::Access => "access",
        }
    }
}

/// Specific event kind within a category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DoorEventKind {
    /// Doorbell ring (HA: `_dispatch_door_event(.., "doorbell", "ring", ..)`).
    DoorbellRing,
    /// Access granted (HA: insights/logs result `"ACCESS"`).
    AccessGranted,
    /// Access denied (any non-ACCESS result).
    AccessDenied,
}

impl DoorEventKind {
    /// Wire-form string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DoorbellRing => "ring",
            Self::AccessGranted => "access_granted",
            Self::AccessDenied => "access_denied",
        }
    }
}

/// A door event dispatched off the WebSocket.
///
/// Source: HA `DoorEvent` dataclass in `coordinator.py`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoorEvent {
    /// Door this event fires for.
    pub door: DoorId,
    /// Event category.
    pub category: DoorEventCategory,
    /// Event kind.
    pub kind: DoorEventKind,
    /// Optional actor display name (HA: `metadata.actor.display_name`).
    pub actor: Option<String>,
    /// Optional authentication provider (HA:
    /// `metadata.authentication.display_name` /
    /// `credential_provider`).
    pub authentication: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_strings() {
        assert_eq!(DoorEventCategory::Doorbell.as_str(), "doorbell");
        assert_eq!(DoorEventCategory::Access.as_str(), "access");
    }

    #[test]
    fn event_kind_strings() {
        assert_eq!(DoorEventKind::DoorbellRing.as_str(), "ring");
        assert_eq!(DoorEventKind::AccessGranted.as_str(), "access_granted");
        assert_eq!(DoorEventKind::AccessDenied.as_str(), "access_denied");
    }
}
