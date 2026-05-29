// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Door + hub domain model (ADR-009).
//!
//! A port of the `UniFi` Access door shape (HA `unifi_access` + the public Access
//! API): a [`AccessDoor`] carries a lock state, a door-position sensor reading,
//! a tamper flag and a relay state; an [`AccessHub`] groups the doors a single
//! reader/controller serves. All of it is pure data — no wire, no hardware.

/// Stable door identifier. Opaque string the controller assigns; the household
/// never sees it (they see [`AccessDoor::name`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DoorId(String);

impl DoorId {
    /// Construct from any string.
    #[must_use]
    pub fn new<S: Into<String>>(raw: S) -> Self {
        Self(raw.into())
    }

    /// Borrow the underlying identifier.
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

/// The lock state of a door's bolt/strike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockState {
    /// The door is mechanically locked.
    Locked,
    /// The door is unlocked and may be opened.
    Unlocked,
    /// The controller has not yet reported a definite state.
    Unknown,
}

/// The door-position sensor (DPS) reading: is the leaf physically open?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoorPosition {
    /// The door leaf is physically open.
    Open,
    /// The door leaf is physically closed.
    Closed,
    /// No position sensor, or it has not reported.
    Unknown,
}

/// The electrical relay that drives the strike. Modelled so a door whose relay
/// is energised but whose bolt is reported locked can be reconciled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayState {
    /// Relay is holding the strike engaged (door secured).
    Engaged,
    /// Relay is released (door can open).
    Released,
}

/// A single controllable door.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessDoor {
    id: DoorId,
    name: String,
    lock: LockState,
    position: DoorPosition,
    relay: RelayState,
    /// True if the position sensor reports it has been physically interfered
    /// with (forced / removed cover).
    tamper: bool,
    /// Whether this door's controller can perform a temporary timed unlock.
    supports_temp_unlock: bool,
}

impl AccessDoor {
    /// Create a door in the safe default: locked, position unknown, relay
    /// engaged, no tamper, temp-unlock supported.
    #[must_use]
    pub fn new<S: Into<String>>(id: DoorId, name: S) -> Self {
        Self {
            id,
            name: name.into(),
            lock: LockState::Locked,
            position: DoorPosition::Unknown,
            relay: RelayState::Engaged,
            tamper: false,
            supports_temp_unlock: true,
        }
    }

    /// Builder: declare whether this door supports timed temporary unlock.
    #[must_use]
    pub fn with_temp_unlock(mut self, supported: bool) -> Self {
        self.supports_temp_unlock = supported;
        self
    }

    /// The door's stable identifier.
    #[must_use]
    pub fn id(&self) -> &DoorId {
        &self.id
    }

    /// The door's household-friendly name, e.g. "Front door".
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Current lock state.
    #[must_use]
    pub fn lock_state(&self) -> LockState {
        self.lock
    }

    /// Current door-position-sensor reading.
    #[must_use]
    pub fn position(&self) -> DoorPosition {
        self.position
    }

    /// Current relay state.
    #[must_use]
    pub fn relay(&self) -> RelayState {
        self.relay
    }

    /// Whether the door is reporting tamper.
    #[must_use]
    pub fn is_tampered(&self) -> bool {
        self.tamper
    }

    /// Whether the controller can perform a timed temporary unlock.
    #[must_use]
    pub fn supports_temp_unlock(&self) -> bool {
        self.supports_temp_unlock
    }

    /// True if the door is physically open right now (DPS reports Open).
    #[must_use]
    pub fn is_physically_open(&self) -> bool {
        matches!(self.position, DoorPosition::Open)
    }

    /// Set the lock state and reconcile the relay (a control-layer helper).
    pub(crate) fn set_lock(&mut self, lock: LockState) {
        self.lock = lock;
        self.relay = match lock {
            LockState::Unlocked => RelayState::Released,
            LockState::Locked | LockState::Unknown => RelayState::Engaged,
        };
    }

    /// Update the door-position-sensor reading (from the controller).
    pub fn set_position(&mut self, position: DoorPosition) {
        self.position = position;
    }

    /// Update the tamper flag (from the controller).
    pub fn set_tamper(&mut self, tampered: bool) {
        self.tamper = tampered;
    }
}

/// A hub / reader that fronts one or more doors.
///
/// The household sees a hub as a place ("Front entrance reader"); cave-home uses
/// it to group the doors a single controller serves so an evacuation/lockdown
/// can sweep all of them at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessHub {
    id: String,
    name: String,
    door_ids: Vec<DoorId>,
}

impl AccessHub {
    /// Create a hub with a name and the doors it serves.
    #[must_use]
    pub fn new<S: Into<String>>(id: S, name: S, door_ids: Vec<DoorId>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            door_ids,
        }
    }

    /// The hub's identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The hub's household-friendly name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The doors this hub serves.
    #[must_use]
    pub fn door_ids(&self) -> &[DoorId] {
        &self.door_ids
    }

    /// Whether this hub serves the given door.
    #[must_use]
    pub fn serves(&self, id: &DoorId) -> bool {
        self.door_ids.iter().any(|d| d == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_door_defaults_to_secure() {
        let d = AccessDoor::new(DoorId::new("d1"), "Front door");
        assert_eq!(d.lock_state(), LockState::Locked);
        assert_eq!(d.position(), DoorPosition::Unknown);
        assert_eq!(d.relay(), RelayState::Engaged);
        assert!(!d.is_tampered());
        assert!(d.supports_temp_unlock());
        assert_eq!(d.name(), "Front door");
        assert_eq!(d.id().as_str(), "d1");
    }

    #[test]
    fn set_lock_reconciles_relay() {
        let mut d = AccessDoor::new(DoorId::new("d1"), "Front door");
        d.set_lock(LockState::Unlocked);
        assert_eq!(d.relay(), RelayState::Released);
        d.set_lock(LockState::Locked);
        assert_eq!(d.relay(), RelayState::Engaged);
        d.set_lock(LockState::Unknown);
        assert_eq!(d.relay(), RelayState::Engaged, "unknown defaults to secure");
    }

    #[test]
    fn position_open_detection() {
        let mut d = AccessDoor::new(DoorId::new("d1"), "Front door");
        assert!(!d.is_physically_open());
        d.set_position(DoorPosition::Open);
        assert!(d.is_physically_open());
        d.set_position(DoorPosition::Closed);
        assert!(!d.is_physically_open());
    }

    #[test]
    fn tamper_flag_toggles() {
        let mut d = AccessDoor::new(DoorId::new("d1"), "Front door");
        d.set_tamper(true);
        assert!(d.is_tampered());
    }

    #[test]
    fn temp_unlock_capability_is_declarable() {
        let d = AccessDoor::new(DoorId::new("g"), "Garage door").with_temp_unlock(false);
        assert!(!d.supports_temp_unlock());
    }

    #[test]
    fn hub_serves_only_its_doors() {
        let hub = AccessHub::new(
            "h1",
            "Front entrance",
            vec![DoorId::new("d1"), DoorId::new("d2")],
        );
        assert!(hub.serves(&DoorId::new("d1")));
        assert!(hub.serves(&DoorId::new("d2")));
        assert!(!hub.serves(&DoorId::new("d3")));
        assert_eq!(hub.name(), "Front entrance");
        assert_eq!(hub.door_ids().len(), 2);
    }
}
