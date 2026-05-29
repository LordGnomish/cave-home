//! `cave-home-air-quality` — air-quality intelligence for cave-home (ADR-019).
//!
//! This crate is the **brain** that turns raw air-quality sensor numbers into a
//! verdict a household can act on: it computes the US EPA Air Quality Index for
//! the AQI pollutants, classifies CO₂ / VOC / radon from their public reference
//! thresholds, and aggregates a room to a single grandma-friendly category with
//! a recommended action in EN / DE / TR.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`reading`] — the vendor-neutral sensor-reading model.
//! - [`aqi`] — the US EPA AQI engine (2024 PM2.5 revision, Charter §7
//!   always-latest).
//! - [`classify`] — CO₂ / VOC-index / radon classifiers.
//! - [`category`] — the six grandma-friendly bands + colour + localised
//!   name/advice (Charter §6.3, ADR-007).
//! - [`assessment`] — worst-of room aggregation, the surface the rest of
//!   cave-home consumes.
//!
//! The **vendor I/O adapters** (AirGradient local API, Awair, IKEA Vindriktning,
//! Airthings) are network/BLE-bound and are deferred to Phase 1b — every one is
//! enumerated in `parity.manifest.toml` `[[unmapped]]` with an ADR-019
//! disposition. They map their wire formats onto [`reading::Reading`] and then
//! reuse this engine unchanged.
//!
//! # Example
//!
//! ```
//! use cave_home_air_quality::{assess, Reading, Pollutant, Lang};
//!
//! let readings = [
//!     Reading::new(Pollutant::Pm25, 20.0).unwrap(),
//!     Reading::new(Pollutant::CarbonDioxide, 1300.0).unwrap(),
//! ];
//! let room = assess(&readings);
//! // Stuffy CO₂ dominates a room that is otherwise only "Fair" on dust.
//! assert_eq!(room.dominant, Some(Pollutant::CarbonDioxide));
//! println!("{}: {}", room.overall.name(Lang::En), room.overall.advice(Lang::En));
//! ```

pub mod aqi;
pub mod assessment;
pub mod category;
pub mod classify;
pub mod nowcast;
pub mod reading;

pub use aqi::{sub_index, AqiOutcome};
pub use assessment::{assess, grade, PollutantGrade, RoomAssessment};
pub use category::{AirCategory, Lang};
pub use classify::classify;
pub use nowcast::now_cast;
pub use reading::{Pollutant, Reading, ReadingError};
