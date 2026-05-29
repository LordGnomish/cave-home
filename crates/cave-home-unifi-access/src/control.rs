// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Door control operations (ADR-009).
//!
//! Pure, deterministic control logic over a set of doors: lock, unlock, a
//! *temporary* unlock that auto-relocks after a caller-supplied duration, and
//! house-wide **evacuation** (all doors unlocked to get out) and **lockdown**
//! (all doors locked to keep people out). It also makes the *door-ajar / held-
//! open* alarm decision: a door open longer than a threshold while it should be
//! closed.
//!
//! Time is supplied by the caller as a monotonic tick (seconds). Nothing here
//! reads a clock, a socket or hardware — a transport layer drives it.

use std::collections::HashMap;

use crate::door::{AccessDoor, DoorId, DoorPosition, LockState};

/// The house-wide emergency mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EmergencyMode {
    /// Normal operation.
    #[default]
    Normal,
    /// Evacuation: every door is held unlocked so people can get out.
    Evacuation,
    /// Lockdown: every door is held locked so nobody gets in.
    Lockdown,
}

/// Why a control operation could not be performed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlError {
    /// No door with this id is known to the controller.
    UnknownDoor(DoorId),
    /// The door does not support a timed temporary unlock.
    TempUnlockUnsupported(DoorId),
    /// A zero-second temporary unlock was requested.
    ZeroDuration,
    /// The operation is refused because the house is locked down.
    LockedDown,
}

impl core::fmt::Display for ControlError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownDoor(id) => write!(f, "unknown door: {id}"),
            Self::TempUnlockUnsupported(id) => {
                write!(f, "door does not support temporary unlock: {id}")
            }
            Self::ZeroDuration => f.write_str("temporary unlock duration must be non-zero"),
            Self::LockedDown => f.write_str("operation refused: house is locked down"),
        }
    }
}

impl std::error::Error for ControlError {}

/// Bookkeeping for a door that is temporarily unlocked and due to re-lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TempUnlock {
    /// Tick at/after which the door re-locks.
    relock_at: u64,
}

/// A controller owning a set of doors and the house emergency mode.
#[derive(Default)]
pub struct AccessController {
    doors: HashMap<DoorId, AccessDoor>,
    temp: HashMap<DoorId, TempUnlock>,
    mode: EmergencyMode,
}

impl AccessController {
    /// A controller with no doors, in normal mode.
    #[must_use]
    pub fn new() -> Self {
        Self {
            doors: HashMap::new(),
            temp: HashMap::new(),
            mode: EmergencyMode::Normal,
        }
    }

    /// Add or replace a door.
    pub fn add_door(&mut self, door: AccessDoor) {
        self.doors.insert(door.id().clone(), door);
    }

    /// Borrow a door by id.
    #[must_use]
    pub fn door(&self, id: &DoorId) -> Option<&AccessDoor> {
        self.doors.get(id)
    }

    /// The current emergency mode.
    #[must_use]
    pub fn mode(&self) -> EmergencyMode {
        self.mode
    }

    /// How many doors are currently temp-unlocked and pending re-lock.
    #[must_use]
    pub fn pending_relocks(&self) -> usize {
        self.temp.len()
    }

    fn door_mut(&mut self, id: &DoorId) -> Result<&mut AccessDoor, ControlError> {
        self.doors
            .get_mut(id)
            .ok_or_else(|| ControlError::UnknownDoor(id.clone()))
    }

    /// Lock a single door. Cancels any pending temporary unlock.
    ///
    /// # Errors
    /// [`ControlError::UnknownDoor`] if the door is not known.
    pub fn lock(&mut self, id: &DoorId) -> Result<(), ControlError> {
        self.door_mut(id)?.set_lock(LockState::Locked);
        self.temp.remove(id);
        Ok(())
    }

    /// Unlock a single door indefinitely. Cancels any pending temporary unlock.
    ///
    /// # Errors
    /// [`ControlError::UnknownDoor`] if the door is not known;
    /// [`ControlError::LockedDown`] if the house is locked down.
    pub fn unlock(&mut self, id: &DoorId) -> Result<(), ControlError> {
        if self.mode == EmergencyMode::Lockdown {
            return Err(ControlError::LockedDown);
        }
        self.door_mut(id)?.set_lock(LockState::Unlocked);
        self.temp.remove(id);
        Ok(())
    }

    /// Temporarily unlock a door for `seconds`, scheduling an auto-relock at
    /// `now + seconds`. The caller later drives [`AccessController::tick`] to
    /// perform the relock.
    ///
    /// # Errors
    /// [`ControlError::UnknownDoor`], [`ControlError::ZeroDuration`],
    /// [`ControlError::TempUnlockUnsupported`], or [`ControlError::LockedDown`].
    pub fn temporary_unlock(
        &mut self,
        id: &DoorId,
        now: u64,
        seconds: u32,
    ) -> Result<(), ControlError> {
        if seconds == 0 {
            return Err(ControlError::ZeroDuration);
        }
        if self.mode == EmergencyMode::Lockdown {
            return Err(ControlError::LockedDown);
        }
        {
            let door = self.door_mut(id)?;
            if !door.supports_temp_unlock() {
                return Err(ControlError::TempUnlockUnsupported(id.clone()));
            }
            door.set_lock(LockState::Unlocked);
        }
        let relock_at = now.saturating_add(u64::from(seconds));
        self.temp.insert(id.clone(), TempUnlock { relock_at });
        Ok(())
    }

    /// Advance time to `now`, re-locking every door whose temporary unlock has
    /// expired. Returns the doors that were re-locked, for the caller to notify
    /// on. Does nothing under evacuation (doors must stay open).
    pub fn tick(&mut self, now: u64) -> Vec<DoorId> {
        if self.mode == EmergencyMode::Evacuation {
            return Vec::new();
        }
        let expired: Vec<DoorId> = self
            .temp
            .iter()
            .filter(|(_, t)| now >= t.relock_at)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &expired {
            if let Some(door) = self.doors.get_mut(id) {
                door.set_lock(LockState::Locked);
            }
            self.temp.remove(id);
        }
        expired
    }

    /// Enter evacuation mode: unlock every door so people can get out, clearing
    /// any pending re-lock.
    pub fn evacuate(&mut self) {
        self.mode = EmergencyMode::Evacuation;
        self.temp.clear();
        for door in self.doors.values_mut() {
            door.set_lock(LockState::Unlocked);
        }
    }

    /// Enter lockdown mode: lock every door so nobody gets in, clearing any
    /// pending re-lock.
    pub fn lockdown(&mut self) {
        self.mode = EmergencyMode::Lockdown;
        self.temp.clear();
        for door in self.doors.values_mut() {
            door.set_lock(LockState::Locked);
        }
    }

    /// Return to normal mode. Doors are left in their current state (the caller
    /// decides whether to re-lock after an evacuation).
    pub fn clear_emergency(&mut self) {
        self.mode = EmergencyMode::Normal;
    }

    /// Decide whether a door has been *held open* too long.
    ///
    /// A door that is physically open, **should be closed** (it is not
    /// deliberately unlocked, or it has simply been ajar past the limit), and
    /// whose `elapsed_open_secs` meets or exceeds `threshold_secs`, raises the
    /// held-open alarm. The caller supplies the elapsed time.
    ///
    /// # Errors
    /// [`ControlError::UnknownDoor`] if the door is not known.
    pub fn is_held_open(
        &self,
        id: &DoorId,
        elapsed_open_secs: u64,
        threshold_secs: u64,
    ) -> Result<bool, ControlError> {
        let door = self
            .doors
            .get(id)
            .ok_or_else(|| ControlError::UnknownDoor(id.clone()))?;
        let open = door.position() == DoorPosition::Open;
        Ok(open && elapsed_open_secs >= threshold_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door::AccessDoor;

    fn controller_with_front() -> (AccessController, DoorId) {
        let mut c = AccessController::new();
        let id = DoorId::new("front");
        c.add_door(AccessDoor::new(id.clone(), "Front door"));
        (c, id)
    }

    #[test]
    fn lock_and_unlock_a_door() {
        let (mut c, id) = controller_with_front();
        c.unlock(&id).expect("unlock");
        assert_eq!(c.door(&id).expect("door").lock_state(), LockState::Unlocked);
        c.lock(&id).expect("lock");
        assert_eq!(c.door(&id).expect("door").lock_state(), LockState::Locked);
    }

    #[test]
    fn unknown_door_errors() {
        let mut c = AccessController::new();
        let err = c.lock(&DoorId::new("nope")).unwrap_err();
        assert!(matches!(err, ControlError::UnknownDoor(_)));
    }

    #[test]
    fn temporary_unlock_then_auto_relock() {
        let (mut c, id) = controller_with_front();
        c.temporary_unlock(&id, 100, 30).expect("temp unlock");
        assert_eq!(c.door(&id).expect("d").lock_state(), LockState::Unlocked);
        assert_eq!(c.pending_relocks(), 1);

        // Before expiry: still unlocked, nothing relocked.
        let relocked = c.tick(129);
        assert!(relocked.is_empty());
        assert_eq!(c.door(&id).expect("d").lock_state(), LockState::Unlocked);
    }

    #[test]
    fn auto_relock_fires_at_boundary() {
        let (mut c, id) = controller_with_front();
        c.temporary_unlock(&id, 100, 30).expect("temp unlock");
        // Exactly at relock_at (130) it re-locks.
        let relocked = c.tick(130);
        assert_eq!(relocked, vec![id.clone()]);
        assert_eq!(c.door(&id).expect("d").lock_state(), LockState::Locked);
        assert_eq!(c.pending_relocks(), 0);
    }

    #[test]
    fn temporary_unlock_rejects_zero_duration() {
        let (mut c, id) = controller_with_front();
        assert_eq!(
            c.temporary_unlock(&id, 0, 0),
            Err(ControlError::ZeroDuration)
        );
    }

    #[test]
    fn temporary_unlock_rejects_unsupported_door() {
        let mut c = AccessController::new();
        let id = DoorId::new("gate");
        c.add_door(AccessDoor::new(id.clone(), "Gate").with_temp_unlock(false));
        assert_eq!(
            c.temporary_unlock(&id, 0, 30),
            Err(ControlError::TempUnlockUnsupported(id.clone()))
        );
    }

    #[test]
    fn manual_lock_cancels_pending_relock() {
        let (mut c, id) = controller_with_front();
        c.temporary_unlock(&id, 100, 30).expect("temp unlock");
        c.lock(&id).expect("manual lock");
        assert_eq!(c.pending_relocks(), 0);
        // A later tick must not re-fire anything.
        assert!(c.tick(200).is_empty());
    }

    #[test]
    fn evacuation_unlocks_all_doors() {
        let mut c = AccessController::new();
        c.add_door(AccessDoor::new(DoorId::new("a"), "Front door"));
        c.add_door(AccessDoor::new(DoorId::new("b"), "Back door"));
        c.evacuate();
        assert_eq!(c.mode(), EmergencyMode::Evacuation);
        assert_eq!(c.door(&DoorId::new("a")).expect("a").lock_state(), LockState::Unlocked);
        assert_eq!(c.door(&DoorId::new("b")).expect("b").lock_state(), LockState::Unlocked);
    }

    #[test]
    fn evacuation_suppresses_auto_relock() {
        let (mut c, id) = controller_with_front();
        c.temporary_unlock(&id, 0, 30).expect("temp");
        c.evacuate();
        // Even well past relock time, evacuation keeps it open.
        assert!(c.tick(10_000).is_empty());
        assert_eq!(c.door(&id).expect("d").lock_state(), LockState::Unlocked);
    }

    #[test]
    fn lockdown_locks_all_and_refuses_unlock() {
        let mut c = AccessController::new();
        c.add_door(AccessDoor::new(DoorId::new("a"), "Front door"));
        c.lockdown();
        assert_eq!(c.mode(), EmergencyMode::Lockdown);
        assert_eq!(c.door(&DoorId::new("a")).expect("a").lock_state(), LockState::Locked);
        // Unlock is refused while locked down.
        assert_eq!(c.unlock(&DoorId::new("a")), Err(ControlError::LockedDown));
        assert_eq!(
            c.temporary_unlock(&DoorId::new("a"), 0, 30),
            Err(ControlError::LockedDown)
        );
    }

    #[test]
    fn clear_emergency_restores_normal_mode() {
        let (mut c, _id) = controller_with_front();
        c.lockdown();
        c.clear_emergency();
        assert_eq!(c.mode(), EmergencyMode::Normal);
    }

    #[test]
    fn held_open_below_threshold_no_alarm() {
        let (mut c, id) = controller_with_front();
        if let Some(d) = c.doors.get_mut(&id) {
            d.set_position(DoorPosition::Open);
        }
        // 29s open, 30s threshold -> no alarm yet.
        assert!(!c.is_held_open(&id, 29, 30).expect("known door"));
    }

    #[test]
    fn held_open_at_threshold_alarms() {
        let (mut c, id) = controller_with_front();
        if let Some(d) = c.doors.get_mut(&id) {
            d.set_position(DoorPosition::Open);
        }
        // Exactly at the threshold the alarm fires.
        assert!(c.is_held_open(&id, 30, 30).expect("known door"));
        assert!(c.is_held_open(&id, 31, 30).expect("known door"));
    }

    #[test]
    fn closed_door_never_alarms() {
        let (mut c, id) = controller_with_front();
        if let Some(d) = c.doors.get_mut(&id) {
            d.set_position(DoorPosition::Closed);
        }
        assert!(!c.is_held_open(&id, 9999, 30).expect("known door"));
    }

    #[test]
    fn held_open_unknown_door_errors() {
        let c = AccessController::new();
        let err = c.is_held_open(&DoorId::new("ghost"), 10, 5).unwrap_err();
        assert!(matches!(err, ControlError::UnknownDoor(_)));
    }
}
