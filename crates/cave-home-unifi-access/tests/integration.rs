// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
//
//! End-to-end integration tests for the cave-home-unifi-access Phase-1 engine:
//! a credential is presented, the policy decides, the controller acts, and the
//! event log records it — exercised across module boundaries.

use cave_home_unifi_access::{
    minute_of_week, AccessController, AccessDoor, AccessEvent, AccessLog, AccessMessage,
    Credential, DenyReason, Direction, DoorId, DoorPosition, EnrolledCredential, Lang, LockState,
    Permission, Policy, Schedule, Window,
};

fn front() -> DoorId {
    DoorId::new("front")
}

fn weekday_business_hours() -> Schedule {
    // Monday 08:00 -> Monday 18:00 only (a cleaner who comes Monday mornings).
    Schedule::from_windows(vec![Window::new(
        minute_of_week(0, 8, 0),
        minute_of_week(0, 18, 0),
    )])
}

#[test]
fn granted_entry_drives_controller_and_log() {
    let mut controller = AccessController::new();
    controller.add_door(AccessDoor::new(front(), "Front door"));

    let mut policy = Policy::new();
    let pin = Credential::pin("1234").expect("valid");
    policy.set_person(
        "alice",
        Permission::new(
            EnrolledCredential::enroll(&pin),
            vec![front()],
            weekday_business_hours(),
        ),
    );

    let mut log = AccessLog::new();

    // Monday 09:00 — inside the window.
    let now = minute_of_week(0, 9, 0);
    let attempt = Credential::pin("1234").expect("valid");
    let decision = policy.decide("alice", &attempt, &front(), now, false);
    assert!(decision.granted);

    // Act: temporarily unlock for 30s, record the entry.
    controller.temporary_unlock(&front(), 1_000, 30).expect("temp unlock");
    log.record(AccessEvent::granted("alice", front(), Direction::Entry, 1_000));
    assert_eq!(
        controller.door(&front()).expect("door").lock_state(),
        LockState::Unlocked
    );

    // 30s later the door auto-relocks.
    let relocked = controller.tick(1_030);
    assert_eq!(relocked, vec![front()]);
    assert_eq!(
        controller.door(&front()).expect("door").lock_state(),
        LockState::Locked
    );

    // The household-facing line is plain language.
    assert_eq!(
        decision.message("Front door").text(Lang::En),
        "Welcome — Front door is open for you."
    );
    assert_eq!(log.granted_for("alice").len(), 1);
}

#[test]
fn denied_outside_hours_is_logged_with_reason() {
    let mut policy = Policy::new();
    let pin = Credential::pin("1234").expect("valid");
    policy.set_person(
        "alice",
        Permission::new(
            EnrolledCredential::enroll(&pin),
            vec![front()],
            weekday_business_hours(),
        ),
    );
    let mut log = AccessLog::new();

    // Monday 22:00 — past the window.
    let now = minute_of_week(0, 22, 0);
    let attempt = Credential::pin("1234").expect("valid");
    let decision = policy.decide("alice", &attempt, &front(), now, false);
    assert_eq!(decision.reason, Some(DenyReason::OutsideSchedule));

    log.record(AccessEvent::denied("alice", front(), DenyReason::OutsideSchedule, 5));
    assert!(!log.events()[0].outcome.is_granted());
    assert_eq!(
        decision.message("Front door").text(Lang::De),
        "Zugang verweigert — außerhalb der erlaubten Zeiten."
    );
}

#[test]
fn lockdown_blocks_everyone_and_controller_locks_all() {
    let mut controller = AccessController::new();
    controller.add_door(AccessDoor::new(front(), "Front door"));
    controller.add_door(AccessDoor::new(DoorId::new("back"), "Back door"));
    controller.lockdown();
    assert_eq!(
        controller.door(&front()).expect("door").lock_state(),
        LockState::Locked
    );

    let mut policy = Policy::new();
    let pin = Credential::pin("1234").expect("valid");
    policy.set_person(
        "alice",
        Permission::new(
            EnrolledCredential::enroll(&pin),
            vec![front()],
            Schedule::always(),
        ),
    );
    // Even valid Alice is refused during lockdown.
    let attempt = Credential::pin("1234").expect("valid");
    let decision = policy.decide("alice", &attempt, &front(), 0, true);
    assert_eq!(decision.reason, Some(DenyReason::LockedDown));
    assert_eq!(decision.message("Front door"), AccessMessage::DeniedLockdown);
}

#[test]
fn held_open_alarm_after_evacuation() {
    let mut controller = AccessController::new();
    let mut door = AccessDoor::new(front(), "Front door");
    door.set_position(DoorPosition::Open);
    controller.add_door(door);
    controller.evacuate();

    // A door left open 45s past a 30s limit raises the held-open alarm.
    assert!(controller.is_held_open(&front(), 45, 30).expect("known door"));
    let msg = AccessMessage::HeldOpen {
        door: "Front door".into(),
    };
    assert_eq!(msg.text(Lang::Tr), "Front door açık kaldı — lütfen kapatın.");
}
