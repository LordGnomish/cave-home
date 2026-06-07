// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The UniFi Access developer-API wire DTOs and their mapping onto the
//! [`cave_home_unifi_access`] domain model.
//!
//! Access does **not** share the Network/Protect console session: it runs its
//! own developer API on a dedicated port (12445) with `Authorization: Bearer`,
//! and its responses use a different envelope — `{ "code": "SUCCESS", "msg",
//! "data" }`. [`AccessEnvelope::into_data`] enforces `code == "SUCCESS"`.
//!
//! Note on door state: the domain [`AccessDoor`] is constructed locked and its
//! live lock state is owned by the door-control engine
//! ([`cave_home_unifi_access::AccessController`]), so the authoritative
//! wire-reported lock state lives on [`DoorStatus::lock`]; [`DoorStatus::into_domain`]
//! produces the door object (carrying the position-sensor reading) that the
//! engine then drives.

use serde::Deserialize;

use cave_home_unifi_access::{
    AccessEvent, AccessOutcome, DenyReason, Direction, DoorId, DoorPosition, LockState,
};

use crate::error::UnifiError;

/// The `{ code, msg, data }` envelope every Access response carries.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessEnvelope<T> {
    /// Result code: `"SUCCESS"` on success, an error token otherwise.
    #[serde(default)]
    pub code: String,
    /// Human message.
    #[serde(default)]
    pub msg: Option<String>,
    /// The payload.
    pub data: Option<T>,
}

impl<T> AccessEnvelope<T> {
    /// Unwrap the data, turning a non-`SUCCESS` code into a [`UnifiError`].
    ///
    /// # Errors
    /// [`UnifiError::Http`] (status 0 — application-level) on a non-`SUCCESS`
    /// code or a missing `data`.
    pub fn into_data(self) -> crate::Result<T> {
        if self.code == "SUCCESS" {
            self.data.ok_or_else(|| UnifiError::Decode("Access response had no data".into()))
        } else {
            Err(UnifiError::Http {
                status: 0,
                message: self.msg.unwrap_or_else(|| format!("Access code {}", self.code)),
                body: String::new(),
            })
        }
    }
}

/// A door as the Access API reports it (`GET /doors`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireDoor {
    /// The door's stable id.
    #[serde(default)]
    pub id: String,
    /// The door's display name.
    #[serde(default)]
    pub name: String,
    /// The fully-qualified name ("Building / Floor / Front door").
    #[serde(default)]
    pub full_name: Option<String>,
    /// The lock-relay status: `"lock"` or `"unlock"`.
    #[serde(default)]
    pub door_lock_relay_status: Option<String>,
    /// The door-position-sensor status: `"open"`, `"close"`, or absent.
    #[serde(default)]
    pub door_position_status: Option<String>,
    /// Whether the door is bound to (served by) a reachable hub.
    #[serde(default)]
    pub is_bind_hub: Option<bool>,
}

/// The cave-home-shaped live door status (the authoritative lock reading).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoorStatus {
    /// Stable id.
    pub id: DoorId,
    /// Display name.
    pub name: String,
    /// Wire-reported lock state.
    pub lock: LockState,
    /// Wire-reported door-position-sensor reading.
    pub position: DoorPosition,
    /// Whether the door's hub is online.
    pub online: bool,
}

impl WireDoor {
    /// Map the lock-relay status string to a domain [`LockState`].
    #[must_use]
    pub fn lock_state(&self) -> LockState {
        match self.door_lock_relay_status.as_deref() {
            Some("lock") => LockState::Locked,
            Some("unlock") => LockState::Unlocked,
            _ => LockState::Unknown,
        }
    }

    /// Map the door-position status string to a domain [`DoorPosition`].
    #[must_use]
    pub fn door_position(&self) -> DoorPosition {
        match self.door_position_status.as_deref() {
            Some("open") => DoorPosition::Open,
            Some("close") => DoorPosition::Closed,
            _ => DoorPosition::Unknown,
        }
    }

    /// Lower to the cave-home [`DoorStatus`].
    #[must_use]
    pub fn into_status(self) -> DoorStatus {
        let name = self
            .full_name
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if self.name.is_empty() {
                    self.id.clone()
                } else {
                    self.name.clone()
                }
            });
        DoorStatus {
            id: DoorId::new(self.id.clone()),
            name,
            lock: self.lock_state(),
            position: self.door_position(),
            online: self.is_bind_hub.unwrap_or(true),
        }
    }
}

/// A visitor / temporary-PIN holder (`GET /visitors`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireVisitor {
    /// Visitor id.
    #[serde(default)]
    pub id: String,
    /// First name.
    #[serde(default)]
    pub first_name: Option<String>,
    /// Last name.
    #[serde(default)]
    pub last_name: Option<String>,
    /// Visit start (unix seconds).
    #[serde(default)]
    pub start_time: Option<i64>,
    /// Visit end (unix seconds).
    #[serde(default)]
    pub end_time: Option<i64>,
}

/// A cave-home-shaped visitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visitor {
    /// Visitor id.
    pub id: String,
    /// Full display name.
    pub name: String,
    /// Visit window start (unix seconds), if set.
    pub start_time: Option<i64>,
    /// Visit window end (unix seconds), if set.
    pub end_time: Option<i64>,
}

impl From<WireVisitor> for Visitor {
    fn from(w: WireVisitor) -> Self {
        let name = format!(
            "{} {}",
            w.first_name.unwrap_or_default(),
            w.last_name.unwrap_or_default()
        )
        .trim()
        .to_string();
        Self {
            id: w.id,
            name,
            start_time: w.start_time,
            end_time: w.end_time,
        }
    }
}

/// An access-log row (`POST /system/logs`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireAccessLog {
    /// The actor (person/visitor) name.
    #[serde(default)]
    pub actor: Option<String>,
    /// The door id the event happened at.
    #[serde(default)]
    pub door_id: Option<String>,
    /// The result text, e.g. `"ACCESS"`, `"BLOCKED"`, `"DENIED"`.
    #[serde(default)]
    pub result: Option<String>,
    /// Whether this passage was an entry or exit, where known.
    #[serde(default)]
    pub direction: Option<String>,
    /// Event time (unix seconds).
    #[serde(default)]
    pub timestamp: Option<u64>,
}

impl WireAccessLog {
    /// Whether the log row represents a granted passage.
    #[must_use]
    pub fn is_granted(&self) -> bool {
        matches!(
            self.result.as_deref(),
            Some("ACCESS" | "GRANTED" | "access" | "granted")
        )
    }

    /// Lower to a domain [`AccessEvent`]. Denied rows map to
    /// [`DenyReason::NoPermission`] (the API does not break the reason down
    /// further on the developer log).
    #[must_use]
    pub fn into_domain(self) -> AccessEvent {
        let who = self.actor.clone().unwrap_or_default();
        let door = DoorId::new(self.door_id.clone().unwrap_or_default());
        let tick = self.timestamp.unwrap_or(0);
        let direction = match self.direction.as_deref() {
            Some("entry" | "in" | "ENTRY") => Direction::Entry,
            Some("exit" | "out" | "EXIT") => Direction::Exit,
            _ => Direction::Unknown,
        };
        let outcome = if self.is_granted() {
            AccessOutcome::Granted
        } else {
            AccessOutcome::Denied(DenyReason::NoPermission)
        };
        AccessEvent {
            who,
            door,
            outcome,
            direction,
            tick,
        }
    }
}

/// The classified kind of a real-time Access notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationKind {
    /// An intercom / doorbell call started at a door (`access.remote_view*`).
    IntercomCall,
    /// A doorbell button press (`access.dps_change` ring, hub bell).
    DoorbellRing,
    /// Someone was granted access.
    AccessGranted,
    /// Someone was denied access.
    AccessDenied,
    /// A device state update (online/offline, lock change).
    DeviceUpdate,
    /// Anything else.
    Other,
}

/// A real-time notification decoded from the Access notifications WebSocket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessNotification {
    /// The raw `event` string from the frame.
    pub event: String,
    /// The door name carried in the payload, if any.
    pub door_name: Option<String>,
    /// The actor name carried in the payload, if any.
    pub actor: Option<String>,
}

impl AccessNotification {
    /// Parse a notification from a raw WebSocket text frame.
    ///
    /// # Errors
    /// [`UnifiError::Decode`] if the frame is not the expected JSON object.
    pub fn parse(frame: &str) -> crate::Result<Self> {
        let v: serde_json::Value = serde_json::from_str(frame)
            .map_err(|e| UnifiError::Decode(format!("access notification: {e}")))?;
        let event = v
            .get("event")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let data = v.get("data").unwrap_or(&serde_json::Value::Null).clone();
        let door_name = data
            .get("door")
            .and_then(|d| d.get("name"))
            .or_else(|| data.get("door_name"))
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        let actor = data
            .get("actor")
            .and_then(|a| a.get("name").or(Some(a)))
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        Ok(Self {
            event,
            door_name,
            actor,
        })
    }

    /// Classify the notification.
    #[must_use]
    pub fn kind(&self) -> NotificationKind {
        let e = self.event.as_str();
        if e.contains("remote_view") || e.contains("intercom") {
            NotificationKind::IntercomCall
        } else if e.contains("dps_change") || e.contains("doorbell") || e.contains("ring") {
            NotificationKind::DoorbellRing
        } else if e.contains("access.logs.add") || e.contains("access.granted") {
            NotificationKind::AccessGranted
        } else if e.contains("denied") || e.contains("blocked") {
            NotificationKind::AccessDenied
        } else if e.contains("device") || e.contains("hub") {
            NotificationKind::DeviceUpdate
        } else {
            NotificationKind::Other
        }
    }

    /// Whether this is an intercom / doorbell call the household should be
    /// woken for.
    #[must_use]
    pub fn is_intercom_call(&self) -> bool {
        self.kind() == NotificationKind::IntercomCall
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_success_unwraps_data() {
        let env: AccessEnvelope<Vec<i32>> =
            serde_json::from_str(r#"{"code":"SUCCESS","msg":"ok","data":[1,2,3]}"#).unwrap();
        assert_eq!(env.into_data().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn envelope_failure_is_error_with_msg() {
        let env: AccessEnvelope<Vec<i32>> = serde_json::from_str(
            r#"{"code":"CODE_ACCESS_TOKEN_INVALID","msg":"token invalid","data":null}"#,
        )
        .unwrap();
        let err = env.into_data().unwrap_err();
        assert!(err.to_string().contains("token invalid"));
    }

    #[test]
    fn wire_door_maps_lock_and_position() {
        let w: WireDoor = serde_json::from_str(
            r#"{"id":"d1","name":"Front","full_name":"Home / Front door",
                "door_lock_relay_status":"unlock","door_position_status":"open",
                "is_bind_hub":true}"#,
        )
        .unwrap();
        assert_eq!(w.lock_state(), LockState::Unlocked);
        assert_eq!(w.door_position(), DoorPosition::Open);
        let s = w.into_status();
        assert_eq!(s.id.as_str(), "d1");
        assert_eq!(s.name, "Home / Front door");
        assert_eq!(s.lock, LockState::Unlocked);
        assert!(s.online);
    }

    #[test]
    fn wire_door_unknown_status_defaults() {
        let w: WireDoor = serde_json::from_str(r#"{"id":"d","name":"Garage"}"#).unwrap();
        assert_eq!(w.lock_state(), LockState::Unknown);
        assert_eq!(w.door_position(), DoorPosition::Unknown);
    }

    #[test]
    fn visitor_name_joins_first_last() {
        let w: WireVisitor = serde_json::from_str(
            r#"{"id":"v1","first_name":"Ada","last_name":"Lovelace","start_time":100,"end_time":200}"#,
        )
        .unwrap();
        let v = Visitor::from(w);
        assert_eq!(v.name, "Ada Lovelace");
        assert_eq!(v.start_time, Some(100));
    }

    #[test]
    fn access_log_granted_maps_to_domain_event() {
        let w: WireAccessLog = serde_json::from_str(
            r#"{"actor":"Burak","door_id":"d1","result":"ACCESS","direction":"entry","timestamp":1717000000}"#,
        )
        .unwrap();
        assert!(w.is_granted());
        let ev = w.into_domain();
        assert_eq!(ev.who, "Burak");
        assert_eq!(ev.door.as_str(), "d1");
        assert_eq!(ev.outcome, AccessOutcome::Granted);
        assert_eq!(ev.direction, Direction::Entry);
        assert_eq!(ev.tick, 1_717_000_000);
    }

    #[test]
    fn access_log_denied_maps_to_no_permission() {
        let w: WireAccessLog = serde_json::from_str(
            r#"{"actor":"Unknown","door_id":"d1","result":"BLOCKED"}"#,
        )
        .unwrap();
        assert!(!w.is_granted());
        assert_eq!(
            w.into_domain().outcome,
            AccessOutcome::Denied(DenyReason::NoPermission)
        );
    }

    #[test]
    fn notification_intercom_call_is_detected() {
        let frame = r#"{"event":"access.remote_view","data":{"door":{"name":"Front door"},"actor":{"name":"Visitor"}}}"#;
        let n = AccessNotification::parse(frame).unwrap();
        assert_eq!(n.kind(), NotificationKind::IntercomCall);
        assert!(n.is_intercom_call());
        assert_eq!(n.door_name.as_deref(), Some("Front door"));
        assert_eq!(n.actor.as_deref(), Some("Visitor"));
    }

    #[test]
    fn notification_classification_variants() {
        let mk = |e: &str| AccessNotification {
            event: e.to_string(),
            door_name: None,
            actor: None,
        };
        assert_eq!(mk("access.dps_change").kind(), NotificationKind::DoorbellRing);
        assert_eq!(mk("access.logs.add").kind(), NotificationKind::AccessGranted);
        assert_eq!(mk("access.denied").kind(), NotificationKind::AccessDenied);
        assert_eq!(mk("access.device.update").kind(), NotificationKind::DeviceUpdate);
        assert_eq!(mk("something.else").kind(), NotificationKind::Other);
    }

    #[test]
    fn notification_parse_rejects_garbage() {
        assert!(AccessNotification::parse("not json").is_err());
    }
}
