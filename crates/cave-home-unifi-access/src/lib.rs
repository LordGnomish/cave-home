// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_precision_loss)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]
//! `cave-home-unifi-access` — door-access control model & policy engine (ADR-009).
//!
//! This crate is the **safety brain** for a UniFi-Access-class door system: it
//! owns the door + hub domain model, the pure control operations (lock, unlock,
//! timed temporary unlock with auto-relock, house-wide evacuation/lockdown, and
//! the held-open alarm decision), the access-credential model, the access-policy
//! engine, the access-event log, and the grandma-friendly EN / DE / TR messages
//! a household reads — all as pure, std-only logic with **no vendor, network,
//! hardware or crypto dependency**.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`door`] — [`AccessDoor`], [`AccessHub`], [`DoorId`], and the
//!   [`LockState`] / [`DoorPosition`] / [`RelayState`] readings, plus tamper.
//! - [`control`] — the [`AccessController`]: lock / unlock / [timed temporary
//!   unlock][`AccessController::temporary_unlock`] with auto-relock,
//!   [`AccessController::evacuate`] / [`AccessController::lockdown`], and the
//!   [held-open alarm][`AccessController::is_held_open`] decision.
//! - [`credential`] — the [`Credential`] value object (PIN / NFC card / mobile /
//!   wave-to-unlock), validated, non-leaking [`Debug`], constant-time digest
//!   compare, and [`EnrolledCredential`] brute-force lock-out.
//! - [`schedule`] — minute-of-week [`Schedule`] / [`Window`] with week-wrap.
//! - [`policy`] — the [`Policy`] engine producing an [`AccessDecision`] with a
//!   [`DenyReason`] (lockdown / unknown / no-permission / outside-schedule).
//! - [`event`] — the [`AccessEvent`] / [`AccessLog`] history + anti-passback
//!   hint.
//! - [`label`] — the [`AccessMessage`] EN / DE / TR strings (Charter §6.3,
//!   ADR-007).
//!
//! The **`UniFi` Access REST + WebSocket transport**, the **hub/reader hardware
//! protocol**, **real cryptographic credential hashing**, the **camera/doorbell
//! pillar tie-in** and **cave-home-core integration** are network / hardware /
//! crypto-bound and deferred to Phase 1b — each is enumerated in
//! `parity.manifest.toml` `[[unmapped]]` with an ADR-009 disposition. cave-home
//! stays cloud-free: only the local Access API is targeted (Charter §9).
//!
//! # Example
//!
//! ```
//! use cave_home_unifi_access::{
//!     AccessController, AccessDoor, DoorId, LockState,
//!     Policy, Permission, Credential, EnrolledCredential,
//!     Schedule, AccessDecision, Lang,
//! };
//!
//! // A front door, locked by default.
//! let front = DoorId::new("front");
//! let mut doors = AccessController::new();
//! doors.add_door(AccessDoor::new(front.clone(), "Front door"));
//!
//! // Alice may use the front door, around the clock, with PIN 1234.
//! let mut policy = Policy::new();
//! let cred = Credential::pin("1234").expect("valid PIN");
//! policy.set_person(
//!     "alice",
//!     Permission::new(EnrolledCredential::enroll(&cred), vec![front.clone()], Schedule::always()),
//! );
//!
//! // She presents her PIN on Monday at 09:00 (minute-of-week 540).
//! let attempt = Credential::pin("1234").expect("valid PIN");
//! let decision: AccessDecision = policy.decide("alice", &attempt, &front, 540, false);
//! assert!(decision.granted);
//!
//! // Let her in for 30 seconds; it auto-relocks on the next tick past expiry.
//! doors.temporary_unlock(&front, 0, 30).expect("temp unlock");
//! assert_eq!(doors.door(&front).unwrap().lock_state(), LockState::Unlocked);
//! let relocked = doors.tick(30);
//! assert_eq!(relocked, vec![front.clone()]);
//!
//! // The household sees plain words, never a protocol or hardware term.
//! let msg = decision.message("Front door");
//! assert_eq!(msg.text(Lang::En), "Welcome — Front door is open for you.");
//! ```

pub mod control;
pub mod credential;
pub mod door;
pub mod event;
pub mod label;
pub mod policy;
pub mod schedule;

pub use control::{AccessController, ControlError, EmergencyMode};
pub use credential::{
    Credential, CredentialDigest, CredentialError, CredentialKind, CredentialVerdict,
    EnrolledCredential,
};
pub use door::{AccessDoor, AccessHub, DoorId, DoorPosition, LockState, RelayState};
pub use event::{AccessEvent, AccessLog, AccessOutcome, Direction};
pub use label::{AccessMessage, Lang};
pub use policy::{AccessDecision, DenyReason, Permission, Policy};
pub use schedule::{minute_of_week, Schedule, Window, MINUTES_PER_WEEK};
