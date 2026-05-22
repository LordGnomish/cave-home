// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//         (tag 2026.5.2) :: homeassistant/components/unifi_access/coordinator.py
//                            + unifi_access_api.{Door, DoorLockRule, DoorLockRuleStatus, ...}

use serde::{Deserialize, Serialize};

use crate::const_table::{
    DEFAULT_LOCK_RULE_INTERVAL, MAX_LOCK_RULE_INTERVAL, MIN_LOCK_RULE_INTERVAL,
};

/// Stable door identifier (HA: `door.id`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DoorId(String);

impl DoorId {
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

impl std::fmt::Display for DoorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Door-relay state (HA: `unifi_access_api.DoorLockRelayStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockRelayStatus {
    /// Relay closed; door is mechanically locked.
    Lock,
    /// Relay open; door is unlocked / can be opened.
    Unlock,
}

impl LockRelayStatus {
    /// Wire-form string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lock => "locked",
            Self::Unlock => "unlocked",
        }
    }

    /// Parse the wire form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "locked" => Some(Self::Lock),
            "unlocked" => Some(Self::Unlock),
            _ => None,
        }
    }
}

/// Door-position state (HA: `LocationUpdateState.dps`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoorPositionStatus {
    /// Door is physically open.
    Open,
    /// Door is physically closed.
    Close,
    /// Position sensor unknown / unsupported.
    Unknown,
}

impl DoorPositionStatus {
    /// Wire-form string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Close => "close",
            Self::Unknown => "unknown",
        }
    }

    /// Parse the wire form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "close" => Some(Self::Close),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// Lock-rule type (HA: `unifi_access_api.DoorLockRuleType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DoorLockRuleType {
    /// Temporarily keep the door locked for `interval` minutes.
    Lock,
    /// Temporarily keep the door unlocked for `interval` minutes.
    Unlock,
    /// Cancel any active temporary rule.
    Reset,
    /// No active rule (resting state).
    None,
}

impl DoorLockRuleType {
    /// Wire-form string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Unlock => "unlock",
            Self::Reset => "reset",
            Self::None => "none",
        }
    }

    /// Parse the wire form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "lock" => Some(Self::Lock),
            "unlock" => Some(Self::Unlock),
            "reset" => Some(Self::Reset),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// Iterate every variant.
    #[must_use]
    pub fn all() -> [Self; 4] {
        [Self::Lock, Self::Unlock, Self::Reset, Self::None]
    }
}

/// Door lock-rule (HA: `unifi_access_api.DoorLockRule`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoorLockRule {
    /// Rule kind.
    pub kind: DoorLockRuleType,
    /// Duration in minutes (1..=480).
    pub interval: u32,
}

impl DoorLockRule {
    /// Construct from raw values; clamps interval to bounds.
    #[must_use]
    pub fn new(kind: DoorLockRuleType, interval: u32) -> Self {
        Self {
            kind,
            interval: Self::clamp_interval(interval),
        }
    }

    /// Clamp interval to [`MIN_LOCK_RULE_INTERVAL`,
    /// `MAX_LOCK_RULE_INTERVAL`].
    #[must_use]
    pub fn clamp_interval(v: u32) -> u32 {
        v.clamp(MIN_LOCK_RULE_INTERVAL, MAX_LOCK_RULE_INTERVAL)
    }

    /// Mirror HA `UnifiAccessCoordinator._normalize_interval` —
    /// `None` -> default; otherwise clamp + bankers'-rounded floor + 0.5.
    #[must_use]
    pub fn normalise_interval(v: Option<f64>) -> u32 {
        let raw = v.unwrap_or(DEFAULT_LOCK_RULE_INTERVAL as f64);
        let clamped = raw
            .max(MIN_LOCK_RULE_INTERVAL as f64)
            .min(MAX_LOCK_RULE_INTERVAL as f64);
        let rounded = (clamped + 0.5).floor() as i64;
        let rounded_u = rounded.max(MIN_LOCK_RULE_INTERVAL as i64) as u32;
        rounded_u.min(MAX_LOCK_RULE_INTERVAL)
    }
}

/// A UniFi Access door.
///
/// Source: HA `unifi_access_api.Door` model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Door {
    /// Stable door identifier.
    pub id: DoorId,
    /// User-set door name.
    pub label: String,
    /// Live lock-relay status.
    pub lock_relay: LockRelayStatus,
    /// Live door-position status (open / close / unknown).
    pub position: DoorPositionStatus,
}

impl Door {
    /// Construct a door in the default "locked, position unknown"
    /// state.
    #[must_use]
    pub fn new(id: DoorId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            lock_relay: LockRelayStatus::Lock,
            position: DoorPositionStatus::Unknown,
        }
    }
}

/// Emergency hub state (HA: `unifi_access_api.EmergencyStatus`).
#[derive(Default, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmergencyStatus {
    /// `evacuation` global rule active (HA settings.update message).
    pub evacuation: bool,
    /// `lockdown` global rule active.
    pub lockdown: bool,
}

impl EmergencyStatus {
    /// True if no emergency rule is active.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        !self.evacuation && !self.lockdown
    }

    /// True if lockdown is on.
    #[must_use]
    pub fn is_lockdown(&self) -> bool {
        self.lockdown
    }

    /// True if evacuation is on.
    #[must_use]
    pub fn is_evacuation(&self) -> bool {
        self.evacuation
    }
}

/// ADR-007 home-world door label. The controller hands us GUIDs +
/// arbitrary user names; the portal renders "Salon kapı", never the
/// GUID.
#[must_use]
pub fn friendly_door_label(user_name: &str) -> String {
    let trimmed = user_name.trim();
    if trimmed.is_empty() {
        "Adsız kapı".to_string()
    } else {
        format!("{trimmed} kapı")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_interval_high() {
        assert_eq!(DoorLockRule::clamp_interval(999), MAX_LOCK_RULE_INTERVAL);
    }

    #[test]
    fn clamp_interval_low() {
        assert_eq!(DoorLockRule::clamp_interval(0), MIN_LOCK_RULE_INTERVAL);
    }

    #[test]
    fn normalise_none_yields_default() {
        assert_eq!(DoorLockRule::normalise_interval(None), DEFAULT_LOCK_RULE_INTERVAL);
    }

    #[test]
    fn normalise_rounds_half_up() {
        assert_eq!(DoorLockRule::normalise_interval(Some(10.5)), 11);
        assert_eq!(DoorLockRule::normalise_interval(Some(10.4)), 10);
    }

    #[test]
    fn emergency_default_clear() {
        assert!(EmergencyStatus::default().is_clear());
    }

    #[test]
    fn friendly_label() {
        assert_eq!(friendly_door_label("Garaj"), "Garaj kapı");
        assert_eq!(friendly_door_label(""), "Adsız kapı");
    }
}
