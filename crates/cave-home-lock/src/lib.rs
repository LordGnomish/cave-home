//! `cave-home-lock` — residential door-lock state model & engine (ADR-016).
//!
//! This crate is the **safety brain** for residential smart locks: it owns the
//! lock state machine (the Home Assistant `lock` entity domain semantics,
//! Apache-2.0), the keypad-PIN credential model, and the grandma-friendly
//! status words a household reads — all as pure, std-only logic with no vendor,
//! radio or network dependency.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`state`] — the [`LockState`] state set and the [`LockCommand`] verbs.
//! - [`machine`] — the [`Lock`] state machine: optimistic apply, confirm/fail
//!   settling, jam handling, capability gating, illegal-transition rejection.
//! - [`code`] — the [`LockCode`] PIN value object and the [`CodeCredential`]
//!   verification contract (no plaintext at rest, constant-time-ish compare,
//!   brute-force lock-out).
//! - [`label`] — EN / DE / TR status label + advice per state (Charter §6.3,
//!   ADR-007).
//!
//! The **vendor I/O adapters** (Nuki, SwitchBot, August/Yale, Aqara, ESPHome,
//! and the Zigbee/Z-Wave/Matter lock-domain bindings), **real cryptographic PIN
//! hashing**, and **cave-home-core entity/state integration** are network /
//! hardware / crypto-bound and deferred to Phase 1b — each is enumerated in
//! `parity.manifest.toml` `[[unmapped]]` with an ADR-016 disposition. They map
//! their wire formats onto this engine and reuse it unchanged.
//!
//! # Example
//!
//! ```
//! use cave_home_lock::{Lock, LockFeatures, LockCommand, LockState, Lang};
//!
//! // A plain front-door deadbolt, currently unlocked.
//! let mut door = Lock::with_state(LockFeatures::deadbolt(), LockState::Unlocked);
//!
//! // Ask it to lock. An optimistic lock reports it is on its way.
//! assert_eq!(door.apply(LockCommand::Lock), Ok(LockState::Locking));
//!
//! // The hardware confirms the bolt is thrown.
//! door.confirm();
//! assert_eq!(door.state(), LockState::Locked);
//!
//! // The household sees plain words, never a vendor or protocol term.
//! assert_eq!(door.state().label(Lang::En), "Door is locked");
//! ```

pub mod code;
pub mod label;
pub mod machine;
pub mod state;

pub use code::{CodeCredential, CodeDigest, CodeError, CodeVerdict, LockCode};
pub use label::Lang;
pub use machine::{Lock, LockFeatures, TransitionError};
pub use state::{LockCommand, LockState};
