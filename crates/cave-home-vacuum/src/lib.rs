//! `cave-home-vacuum` — robot-vacuum state model & engine (ADR-017).
//!
//! This crate is the **brain** for a cloud-free robot vacuum: it owns the vacuum
//! state machine (the Home Assistant `vacuum` entity domain + Valetudo control
//! surface semantics, both permissive), the suction-power model, the battery and
//! low-battery auto-return logic, the room/zone clean-request model with map
//! validation, the fault taxonomy, and the grandma-friendly status words a
//! household reads — all as pure, std-only logic with no vendor, radio or
//! network dependency.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`state`] — the [`VacuumState`] state set and the [`VacuumCommand`] verbs.
//! - [`machine`] — the [`Vacuum`] state machine: command application, illegal-
//!   transition rejection, capability gating, low-battery auto-return, fault
//!   surfacing and gating, and clearing.
//! - [`fan`] — the [`FanSpeed`] presets and per-vacuum [`FanCapability`] gating.
//! - [`battery`] — the [`Battery`] value type and auto-return threshold logic.
//! - [`map`] — the [`Segment`] / [`Zone`] value types and the validated
//!   clean-segments request ([`VacuumMap::validate_segments`]).
//! - [`error`] — the [`ErrorCode`] fault taxonomy.
//! - [`label`] — EN / DE / TR status label + fault explanation/advice (Charter
//!   §6.3, ADR-007).
//!
//! The **vendor I/O adapters** (Valetudo REST/MQTT, the Roborock / Dreame / etc.
//! vendor-cloud paths), **live map / lidar rendering**, and **cave-home-core
//! entity/state integration** are network / hardware bound and deferred to
//! Phase 1b — each is enumerated in `parity.manifest.toml` `[[unmapped]]` with an
//! ADR-017 disposition. They map their wire formats onto this engine and reuse
//! it unchanged. cave-home does not re-flash vacuum firmware (ADR-017).
//!
//! # Example
//!
//! ```
//! use cave_home_vacuum::{
//!     Battery, ChargeDirection, Lang, Vacuum, VacuumCommand, VacuumFeatures, VacuumState,
//! };
//!
//! // A map-aware vacuum, resting on its dock, well charged.
//! let battery = Battery::new(90, ChargeDirection::Discharging).unwrap();
//! let mut robot = Vacuum::with_state(VacuumFeatures::full(), VacuumState::Docked, battery);
//!
//! // Ask it to start cleaning.
//! assert_eq!(robot.apply(VacuumCommand::Start), Ok(VacuumState::Cleaning));
//!
//! // The household sees plain words, never a vendor or protocol term.
//! assert_eq!(robot.state().label(Lang::En), "Vacuum is cleaning");
//!
//! // Battery drops low mid-clean: it heads home on its own.
//! let low = Battery::new(15, ChargeDirection::Discharging).unwrap();
//! assert!(robot.update_battery(low));
//! assert_eq!(robot.state(), VacuumState::Returning);
//! ```

pub mod battery;
pub mod error;
pub mod fan;
pub mod label;
pub mod machine;
pub mod map;
pub mod state;

pub use battery::{Battery, BatteryError, ChargeDirection, DEFAULT_RETURN_THRESHOLD};
pub use error::ErrorCode;
pub use fan::{FanCapability, FanSpeed};
pub use label::Lang;
pub use machine::{CommandError, Vacuum, VacuumFeatures};
pub use map::{Segment, SegmentRequestError, VacuumMap, Zone, ZoneError};
pub use state::{VacuumCommand, VacuumState};
