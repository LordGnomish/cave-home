// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Access-policy engine (ADR-009).
//!
//! A [`Policy`] maps each person to the doors they may use and the [`Schedule`]
//! they may use them on. Given a presented credential, a target door, the
//! current minute-of-week and whether the house is locked down, the engine
//! returns a single [`AccessDecision`] carrying `granted` plus a machine-
//! readable `reason`. The reason maps straight onto a grandma-friendly message
//! in [`crate::label`].
//!
//! Decision order (safety first):
//! 1. **Lockdown** overrides everything — nobody but an evacuation override gets
//!    through a locked-down house.
//! 2. **Unknown credential** — the credential is not enrolled at all.
//! 3. **No permission** — enrolled, but not for this door.
//! 4. **Outside schedule** — allowed for this door, but not at this hour.
//! 5. **Granted**.

use std::collections::HashMap;

use crate::credential::{Credential, CredentialVerdict, EnrolledCredential};
use crate::door::DoorId;
use crate::label::AccessMessage;
use crate::schedule::Schedule;

/// Why an access request was refused. Mirrors the safety decision order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DenyReason {
    /// The house is in lockdown; nobody passes.
    LockedDown,
    /// The presented credential is not enrolled for any person.
    UnknownCredential,
    /// Enrolled, but this person may not use this door.
    NoPermission,
    /// Allowed for this door, but not at the current time.
    OutsideSchedule,
}

/// The engine's verdict for one access request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessDecision {
    /// Whether the person is let through.
    pub granted: bool,
    /// `None` when granted; otherwise the refusal reason.
    pub reason: Option<DenyReason>,
}

impl AccessDecision {
    /// A grant.
    #[must_use]
    pub fn granted() -> Self {
        Self {
            granted: true,
            reason: None,
        }
    }

    /// A denial with a reason.
    #[must_use]
    pub fn denied(reason: DenyReason) -> Self {
        Self {
            granted: false,
            reason: Some(reason),
        }
    }

    /// Map this decision to a household-facing message for the given door name.
    #[must_use]
    pub fn message(&self, door_name: &str) -> AccessMessage {
        match self.reason {
            None => AccessMessage::AccessGranted {
                door: door_name.to_string(),
            },
            Some(DenyReason::LockedDown) => AccessMessage::DeniedLockdown,
            Some(DenyReason::UnknownCredential) => AccessMessage::DeniedUnknown,
            Some(DenyReason::NoPermission) => AccessMessage::DeniedNoPermission,
            Some(DenyReason::OutsideSchedule) => AccessMessage::DeniedOutsideHours,
        }
    }
}

/// What one person is permitted: the doors they may use and on what schedule.
#[derive(Clone, Debug)]
pub struct Permission {
    /// The enrolled credential the person presents.
    pub credential: EnrolledCredential,
    /// The doors this person may use.
    pub doors: Vec<DoorId>,
    /// The schedule this person may use those doors on.
    pub schedule: Schedule,
}

impl Permission {
    /// Build a permission.
    #[must_use]
    pub fn new(credential: EnrolledCredential, doors: Vec<DoorId>, schedule: Schedule) -> Self {
        Self {
            credential,
            doors,
            schedule,
        }
    }

    /// Whether this permission covers the given door.
    #[must_use]
    pub fn covers_door(&self, door: &DoorId) -> bool {
        self.doors.iter().any(|d| d == door)
    }
}

/// The access policy: a set of per-person permissions.
#[derive(Default)]
pub struct Policy {
    people: HashMap<String, Permission>,
}

impl Policy {
    /// An empty policy (nobody is enrolled).
    #[must_use]
    pub fn new() -> Self {
        Self {
            people: HashMap::new(),
        }
    }

    /// Enroll or replace a person's permission.
    pub fn set_person(&mut self, person: impl Into<String>, permission: Permission) {
        self.people.insert(person.into(), permission);
    }

    /// How many people are enrolled.
    #[must_use]
    pub fn len(&self) -> usize {
        self.people.len()
    }

    /// Whether nobody is enrolled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.people.is_empty()
    }

    /// Decide whether `person` may pass through `door` right now.
    ///
    /// The presented `credential` is verified against the person's enrolled
    /// credential (a wrong or wrong-kind credential is `UnknownCredential` from
    /// the door's point of view — the engine never reveals *who* a credential
    /// would have matched). `locked_down` reflects a house-wide lockdown.
    ///
    /// Verifying mutably advances the enrolled credential's brute-force counter,
    /// so this takes `&mut self`.
    pub fn decide(
        &mut self,
        person: &str,
        credential: &Credential,
        door: &DoorId,
        minute_of_week: u32,
        locked_down: bool,
    ) -> AccessDecision {
        // 1. Lockdown overrides everything.
        if locked_down {
            return AccessDecision::denied(DenyReason::LockedDown);
        }

        let Some(perm) = self.people.get_mut(person) else {
            return AccessDecision::denied(DenyReason::UnknownCredential);
        };

        // 2. The credential must actually match this person's enrolment.
        match perm.credential.verify(credential) {
            CredentialVerdict::Accepted => {}
            CredentialVerdict::Rejected | CredentialVerdict::LockedOut => {
                return AccessDecision::denied(DenyReason::UnknownCredential);
            }
        }

        // 3. The person must be permitted on this door.
        if !perm.covers_door(door) {
            return AccessDecision::denied(DenyReason::NoPermission);
        }

        // 4. The current time must fall inside the schedule.
        if !perm.schedule.allows(minute_of_week) {
            return AccessDecision::denied(DenyReason::OutsideSchedule);
        }

        // 5. Granted.
        AccessDecision::granted()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::Credential;
    use crate::label::Lang;
    use crate::schedule::{minute_of_week, Schedule, Window};

    fn enrolled_pin(pin: &str) -> EnrolledCredential {
        let c = Credential::pin(pin).expect("valid pin");
        EnrolledCredential::enroll(&c)
    }

    fn front_door() -> DoorId {
        DoorId::new("front")
    }

    fn business_hours() -> Schedule {
        // Monday 08:00 -> Monday 18:00.
        Schedule::from_windows(vec![Window::new(
            minute_of_week(0, 8, 0),
            minute_of_week(0, 18, 0),
        )])
    }

    fn policy_with_alice() -> Policy {
        let mut p = Policy::new();
        p.set_person(
            "alice",
            Permission::new(enrolled_pin("1234"), vec![front_door()], business_hours()),
        );
        p
    }

    #[test]
    fn granted_when_everything_lines_up() {
        let mut p = policy_with_alice();
        let cred = Credential::pin("1234").expect("valid");
        let d = p.decide("alice", &cred, &front_door(), minute_of_week(0, 9, 0), false);
        assert!(d.granted);
        assert_eq!(d.reason, None);
    }

    #[test]
    fn lockdown_overrides_a_valid_request() {
        let mut p = policy_with_alice();
        let cred = Credential::pin("1234").expect("valid");
        let d = p.decide("alice", &cred, &front_door(), minute_of_week(0, 9, 0), true);
        assert!(!d.granted);
        assert_eq!(d.reason, Some(DenyReason::LockedDown));
    }

    #[test]
    fn unknown_person_is_unknown_credential() {
        let mut p = policy_with_alice();
        let cred = Credential::pin("1234").expect("valid");
        let d = p.decide("mallory", &cred, &front_door(), minute_of_week(0, 9, 0), false);
        assert_eq!(d.reason, Some(DenyReason::UnknownCredential));
    }

    #[test]
    fn wrong_pin_is_unknown_credential() {
        let mut p = policy_with_alice();
        let cred = Credential::pin("9999").expect("valid");
        let d = p.decide("alice", &cred, &front_door(), minute_of_week(0, 9, 0), false);
        assert_eq!(d.reason, Some(DenyReason::UnknownCredential));
    }

    #[test]
    fn wrong_kind_is_unknown_credential() {
        let mut p = policy_with_alice();
        let card = Credential::nfc_card("0a1b2c3d").expect("valid");
        let d = p.decide("alice", &card, &front_door(), minute_of_week(0, 9, 0), false);
        assert_eq!(d.reason, Some(DenyReason::UnknownCredential));
    }

    #[test]
    fn no_permission_for_unlisted_door() {
        let mut p = policy_with_alice();
        let cred = Credential::pin("1234").expect("valid");
        let back = DoorId::new("back");
        let d = p.decide("alice", &cred, &back, minute_of_week(0, 9, 0), false);
        assert_eq!(d.reason, Some(DenyReason::NoPermission));
    }

    #[test]
    fn outside_schedule_is_denied() {
        let mut p = policy_with_alice();
        let cred = Credential::pin("1234").expect("valid");
        // Monday 20:00 is past the 18:00 window end.
        let d = p.decide("alice", &cred, &front_door(), minute_of_week(0, 20, 0), false);
        assert_eq!(d.reason, Some(DenyReason::OutsideSchedule));
    }

    #[test]
    fn lockdown_takes_priority_over_unknown() {
        // Even an unknown person gets the lockdown reason, not unknown-credential.
        let mut p = policy_with_alice();
        let cred = Credential::pin("0000").expect("valid");
        let d = p.decide("nobody", &cred, &front_door(), minute_of_week(0, 9, 0), true);
        assert_eq!(d.reason, Some(DenyReason::LockedDown));
    }

    #[test]
    fn decision_maps_to_messages() {
        assert_eq!(
            AccessDecision::granted().message("Front door"),
            AccessMessage::AccessGranted { door: "Front door".into() }
        );
        assert_eq!(
            AccessDecision::denied(DenyReason::OutsideSchedule).message("Front door"),
            AccessMessage::DeniedOutsideHours
        );
        assert_eq!(
            AccessDecision::denied(DenyReason::LockedDown).message("Front door"),
            AccessMessage::DeniedLockdown
        );
        // And the message renders in a household language.
        let m = AccessDecision::denied(DenyReason::NoPermission).message("Front door");
        assert!(m.text(Lang::Tr).contains("izniniz"));
    }

    #[test]
    fn policy_membership_helpers() {
        let p = policy_with_alice();
        assert_eq!(p.len(), 1);
        assert!(!p.is_empty());
        let empty = Policy::new();
        assert!(empty.is_empty());
    }

    #[test]
    fn always_schedule_grants_any_time() {
        let mut p = Policy::new();
        p.set_person(
            "owner",
            Permission::new(enrolled_pin("4321"), vec![front_door()], Schedule::always()),
        );
        let cred = Credential::pin("4321").expect("valid");
        let d = p.decide("owner", &cred, &front_door(), minute_of_week(6, 3, 0), false);
        assert!(d.granted, "24/7 access should grant at Sunday 03:00");
    }
}
